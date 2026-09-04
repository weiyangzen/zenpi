//! Bounded owner implementations for slash commands that inspect the
//! workspace or stage input for the next provider turn.
//!
//! Slash parsing stays deliberately side-effect free.  This module is the
//! small host-facing boundary used by both the TUI and JSONL dispatchers:
//! paths are resolved below the current workspace, symlinks are rejected,
//! output is bounded, and attachment bytes are materialized only by the
//! [`crate::core::Agent`] when a turn is admitted.

use std::{
    fs,
    io::{self, Read},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    backend::{AttachmentKind, InputAttachment, MAX_ATTACHMENT_BYTES},
    core::{Agent, AgentError},
};

/// Keep a slash-command diff small enough for a terminal frame and a JSONL
/// response cache.  This is intentionally independent from tool result caps:
/// a review command must remain cheap even when a repository has a huge file.
pub const MAX_SLASH_DIFF_BYTES: usize = 64 * 1024;
/// `git status` is metadata only, but it can still contain one line per file.
pub const MAX_STATUS_BYTES: usize = 32 * 1024;
const MAX_WORKSPACE_PATH_BYTES: usize = 4096;
const DIFF_TRUNCATION_MARKER: &str = "[diff truncated]";

#[derive(Debug, Error)]
pub enum SlashActionError {
    #[error("workspace path denied: {0}")]
    PathDenied(String),
    #[error("workspace path is not a regular file: {0}")]
    NotAFile(String),
    #[error("workspace file is too large (maximum {max} bytes): {path}")]
    FileTooLarge { path: String, max: usize },
    #[error("workspace file metadata failed: {0}")]
    Metadata(#[source] io::Error),
    #[error("workspace file read failed: {0}")]
    Read(#[source] io::Error),
    #[error("git diff failed: {0}")]
    Git(String),
    #[error("agent attachment staging failed: {0}")]
    Agent(#[from] AgentError),
    #[error("slash action serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Inspect the pending changes in the process workspace.  The optional path
/// is workspace-relative; omitting it reviews the complete repository diff.
/// Both staged and unstaged tracked changes are included (`HEAD` diff), while
/// an explicitly requested untracked file receives a synthetic `/dev/null`
/// diff so `/diff new-file.txt` is useful before the first `git add`.
pub fn diff_value(path: Option<&str>) -> Result<Value, SlashActionError> {
    let workspace = workspace_root()?;
    diff_value_at(&workspace, path)
}

/// Resolve `/diff` against an agent's configured workspace when one is
/// available, falling back to the process workspace for a busy host that
/// cannot borrow the agent lock. This keeps TUI/headless path semantics in
/// sync with attachment materialization.
pub fn diff_value_for_agent(
    agent: Option<&Agent>,
    path: Option<&str>,
) -> Result<Value, SlashActionError> {
    match agent.and_then(|agent| agent.attachment_workspace_root()) {
        Some(root) => diff_value_at(root, path),
        None => diff_value(path),
    }
}

/// Testable form of [`diff_value`] with an explicit workspace root.  Hosts
/// normally use [`diff_value`], which anchors the command to their cwd.
pub fn diff_value_at(
    workspace_root: impl AsRef<Path>,
    path: Option<&str>,
) -> Result<Value, SlashActionError> {
    let workspace = canonical_workspace(workspace_root.as_ref())?;
    let relative = match path {
        Some(raw) => Some(resolve_relative(&workspace, raw, true)?),
        None => None,
    };

    let status = git_status(&workspace, relative.as_deref())?;
    if let Some(target) = relative.as_deref()
        && !target.exists()
        && status.trim().is_empty()
    {
        return Err(SlashActionError::PathDenied(path_display(target)));
    }
    let untracked = relative.as_deref().is_some_and(|target| {
        target.is_file()
            && status
                .lines()
                .any(|line| line.starts_with("?? ") || line.starts_with("??\t"))
    });

    let (diff, exit_status, process_truncated) = if untracked {
        let target = relative
            .as_deref()
            .ok_or_else(|| SlashActionError::PathDenied("missing diff path".into()))?;
        let target_name = format!(
            "./{}",
            path_display(
                target
                    .strip_prefix(&workspace)
                    .map_err(|_| SlashActionError::PathDenied(path_display(target)))?,
            )
        );
        git_diff_process(
            &workspace,
            &["diff", "--no-index", "--no-color", "/dev/null"],
            Some(Path::new(&target_name)),
        )?
    } else {
        let mut args = vec![
            "diff",
            "HEAD",
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            "--unified=3",
            "--",
        ];
        // Keep the path as a separate argument.  It is never interpreted by a
        // shell, and the preceding `--` prevents option injection.
        let mut owned_path;
        if let Some(relative) = relative.as_deref() {
            owned_path = relative
                .strip_prefix(&workspace)
                .map_err(|_| SlashActionError::PathDenied(path_display(relative)))
                .map(path_display)?;
            // `Path::strip_prefix` returns an empty path for `/diff .`, but
            // Git treats an empty pathspec as invalid; normalize it to `.`.
            if owned_path.is_empty() {
                owned_path.push('.');
            }
            args.push(&owned_path);
        }
        git_diff_process(&workspace, &args, None)?
    };

    // `git diff --no-index` exits 1 when differences exist.  All other
    // non-zero statuses are genuine failures and should be visible to the
    // owner rather than presented as an empty successful review.
    if exit_status != 0 && !process_truncated && !(untracked && exit_status == 1) {
        return Err(SlashActionError::Git(format!(
            "git diff exited with status {exit_status}"
        )));
    }
    let (diff, bounded_truncated) = bound_text(diff);
    let truncated = process_truncated || bounded_truncated;
    let relative_display = relative
        .as_deref()
        .map(|absolute| {
            let display = absolute
                .strip_prefix(&workspace)
                .map(path_display)
                .unwrap_or_else(|_| path_display(absolute));
            if display.is_empty() {
                ".".into()
            } else {
                display
            }
        })
        .unwrap_or_else(|| ".".into());
    Ok(json!({
        "command": "diff",
        "source": "git",
        "route": "local",
        "accepted": true,
        "path": relative_display,
        "changed": !diff.trim().is_empty(),
        "diff": diff,
        "truncated": truncated,
        "status": bound_status(status),
    }))
}

/// Validate and stage one workspace file on the agent for its next admitted
/// turn.  The agent re-reads and hashes the bytes at admission time; this
/// command only records a bounded reference and never writes file contents to
/// the journal.
pub fn attach_value(agent: &mut Agent, path: &str) -> Result<Value, SlashActionError> {
    let workspace = agent
        .attachment_workspace_root()
        .map(PathBuf::from)
        .map(Ok)
        .unwrap_or_else(workspace_root)?;
    attach_value_at(agent, &workspace, path)
}

/// Testable form of [`attach_value`] with an explicit workspace root. The
/// supplied root should match the `ToolContext` configured on `agent`; the
/// agent still performs its own bounded revalidation before retaining it.
pub fn attach_value_at(
    agent: &mut Agent,
    workspace_root: impl AsRef<Path>,
    path: &str,
) -> Result<Value, SlashActionError> {
    let workspace = canonical_workspace(workspace_root.as_ref())?;
    let attachment = attachment_for_path(&workspace, path)?;
    let bytes = attachment_size(&workspace, attachment.path.as_deref().unwrap_or(path))?;
    let digest = file_digest(&workspace, attachment.path.as_deref().unwrap_or(path))?;
    agent.stage_attachment(attachment.clone())?;
    Ok(json!({
        "command": "attach",
        "accepted": true,
        "path": attachment.path,
        "kind": attachment.kind,
        "mime_type": attachment.mime_type,
        "size_bytes": bytes,
        "sha256": digest,
        "pending": agent.pending_attachments().len(),
        "message": "attachment staged for the next turn",
    }))
}

/// Testable attachment constructor.  It performs all path and size checks but
/// does not mutate an agent.
pub fn attachment_for_path(
    workspace_root: impl AsRef<Path>,
    path: &str,
) -> Result<InputAttachment, SlashActionError> {
    let workspace = canonical_workspace(workspace_root.as_ref())?;
    let resolved = resolve_relative(&workspace, path, false)?;
    let metadata = fs::symlink_metadata(&resolved).map_err(|error| map_path_error(path, error))?;
    if !metadata.is_file() {
        return Err(SlashActionError::NotAFile(path.to_owned()));
    }
    if metadata.len() > MAX_ATTACHMENT_BYTES as u64 {
        return Err(SlashActionError::FileTooLarge {
            path: path.to_owned(),
            max: MAX_ATTACHMENT_BYTES,
        });
    }
    let relative = resolved
        .strip_prefix(&workspace)
        .map_err(|_| SlashActionError::PathDenied(path.to_owned()))?;
    let relative = path_display(relative);
    let mime_type = mime_for_path(&resolved);
    let kind = if mime_type.starts_with("image/") {
        AttachmentKind::Image
    } else {
        AttachmentKind::File
    };
    Ok(InputAttachment {
        kind,
        mime_type,
        path: Some(relative),
        url: None,
        file_id: None,
    })
}

fn workspace_root() -> Result<PathBuf, SlashActionError> {
    let cwd = std::env::current_dir().map_err(SlashActionError::Metadata)?;
    canonical_workspace(&cwd)
}

fn canonical_workspace(path: &Path) -> Result<PathBuf, SlashActionError> {
    let root = path
        .canonicalize()
        .map_err(|error| map_path_error(&path.display().to_string(), error))?;
    let metadata = fs::symlink_metadata(&root).map_err(SlashActionError::Metadata)?;
    if !metadata.is_dir() {
        return Err(SlashActionError::PathDenied(format!(
            "workspace is not a directory: {}",
            path.display()
        )));
    }
    Ok(root)
}

/// Resolve a relative path while rejecting every symbolic-link component.  A
/// missing final component is allowed only when `allow_missing_final` is true;
/// `/diff` allows a missing final component so a deleted tracked path can be
/// reviewed, while parent components are always checked so it cannot escape
/// through a link.
fn resolve_relative(
    workspace: &Path,
    raw: &str,
    allow_missing_final: bool,
) -> Result<PathBuf, SlashActionError> {
    if raw.trim().is_empty()
        || raw.len() > MAX_WORKSPACE_PATH_BYTES
        || raw.chars().any(char::is_control)
    {
        return Err(SlashActionError::PathDenied(raw.to_owned()));
    }
    let relative = Path::new(raw);
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(SlashActionError::PathDenied(raw.to_owned()));
    }
    let mut cursor = workspace.to_path_buf();
    let components = relative.components().collect::<Vec<_>>();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(part) = component else {
            continue;
        };
        cursor.push(part);
        match fs::symlink_metadata(&cursor) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(SlashActionError::PathDenied(
                    "workspace path contains a symbolic link".into(),
                ));
            }
            Ok(_) => {}
            Err(error)
                if error.kind() == io::ErrorKind::NotFound
                    && allow_missing_final
                    && index + 1 == components.len() =>
            {
                return Ok(cursor);
            }
            Err(error) => return Err(map_path_error(raw, error)),
        }
    }
    let canonical = cursor
        .canonicalize()
        .map_err(|error| map_path_error(raw, error))?;
    if !canonical.starts_with(workspace) {
        return Err(SlashActionError::PathDenied(raw.to_owned()));
    }
    Ok(canonical)
}

fn map_path_error(path: &str, error: io::Error) -> SlashActionError {
    if error.kind() == io::ErrorKind::NotFound {
        SlashActionError::PathDenied(path.to_owned())
    } else {
        SlashActionError::Metadata(error)
    }
}

fn path_display(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn mime_for_path(path: &Path) -> String {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "txt" | "log" | "md" | "markdown" => "text/plain",
        "json" => "application/json",
        "toml" => "application/toml",
        "yaml" | "yml" => "application/yaml",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "rs" | "c" | "h" | "cpp" | "js" | "ts" | "py" | "sh" => "text/plain",
        _ => "application/octet-stream",
    }
    .into()
}

fn attachment_size(workspace: &Path, path: &str) -> Result<u64, SlashActionError> {
    let resolved = resolve_relative(workspace, path, false)?;
    let metadata = fs::metadata(resolved).map_err(SlashActionError::Metadata)?;
    Ok(metadata.len())
}

fn file_digest(workspace: &Path, path: &str) -> Result<String, SlashActionError> {
    let resolved = resolve_relative(workspace, path, false)?;
    let mut file = fs::File::open(resolved).map_err(SlashActionError::Read)?;
    let mut hasher = Sha256::new();
    let mut total = 0usize;
    let mut buffer = [0u8; 16 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(SlashActionError::Read)?;
        if read == 0 {
            break;
        }
        total = total.saturating_add(read);
        if total > MAX_ATTACHMENT_BYTES {
            return Err(SlashActionError::FileTooLarge {
                path: path.to_owned(),
                max: MAX_ATTACHMENT_BYTES,
            });
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn git_status(workspace: &Path, path: Option<&Path>) -> Result<String, SlashActionError> {
    let mut command = Command::new("git");
    command
        .current_dir(workspace)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_PAGER", "cat")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdout(Stdio::piped())
        // Diagnostics are intentionally discarded: retaining stderr would
        // create a second untrusted pipe that could deadlock or grow while a
        // hostile repository emits a large filter error.
        .stderr(Stdio::null())
        .args(["status", "--porcelain=v1", "--untracked-files=normal", "--"]);
    if let Some(path) = path {
        let relative = path
            .strip_prefix(workspace)
            .map_err(|_| SlashActionError::PathDenied(path_display(path)))?;
        command.arg(if relative.as_os_str().is_empty() {
            Path::new(".")
        } else {
            relative
        });
    }
    let mut child = command
        .spawn()
        .map_err(|error| SlashActionError::Git(error.to_string()))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| SlashActionError::Git("git status stdout unavailable".into()))?;
    let mut bytes = Vec::new();
    stdout
        .by_ref()
        .take((MAX_STATUS_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(SlashActionError::Read)?;
    let output_truncated = bytes.len() > MAX_STATUS_BYTES;
    if output_truncated {
        let _ = child.kill();
    }
    let status = child
        .wait()
        .map_err(SlashActionError::Read)?
        .code()
        .unwrap_or(-1);
    if !output_truncated && status != 0 {
        return Err(SlashActionError::Git(format!(
            "git status exited with status {status}"
        )));
    }
    Ok(bound_status(String::from_utf8_lossy(&bytes).into_owned()))
}

/// Run a git process with a hard stdout read bound.  The child is killed as
/// soon as stdout exceeds the cap, preventing a gigantic diff from becoming
/// an unbounded allocation or blocking the host indefinitely.
fn git_diff_process(
    workspace: &Path,
    args: &[&str],
    extra_path: Option<&Path>,
) -> Result<(String, i32, bool), SlashActionError> {
    let mut command = Command::new("git");
    command
        .current_dir(workspace)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_PAGER", "cat")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .args(args);
    if let Some(path) = extra_path {
        command.arg(path);
    }
    let mut child = command
        .spawn()
        .map_err(|error| SlashActionError::Git(error.to_string()))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| SlashActionError::Git("git stdout unavailable".into()))?;
    let mut bytes = Vec::new();
    stdout
        .by_ref()
        .take((MAX_SLASH_DIFF_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(SlashActionError::Read)?;
    let truncated = bytes.len() > MAX_SLASH_DIFF_BYTES;
    if truncated {
        let _ = child.kill();
    }
    let status = child
        .wait()
        .map_err(SlashActionError::Read)?
        .code()
        .unwrap_or(-1);
    if truncated {
        bytes.truncate(MAX_SLASH_DIFF_BYTES);
        bytes.extend_from_slice(DIFF_TRUNCATION_MARKER.as_bytes());
    }
    Ok((
        String::from_utf8_lossy(&bytes).into_owned(),
        status,
        truncated,
    ))
}

fn bound_text(mut text: String) -> (String, bool) {
    if text.len() <= MAX_SLASH_DIFF_BYTES {
        return (text, false);
    }
    let mut end = MAX_SLASH_DIFF_BYTES.saturating_sub(DIFF_TRUNCATION_MARKER.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text.push_str(DIFF_TRUNCATION_MARKER);
    (text, true)
}

fn bound_status(mut status: String) -> String {
    if status.len() <= MAX_STATUS_BYTES {
        return status;
    }
    let mut end = MAX_STATUS_BYTES.saturating_sub(DIFF_TRUNCATION_MARKER.len());
    while end > 0 && !status.is_char_boundary(end) {
        end -= 1;
    }
    status.truncate(end);
    status.push_str(DIFF_TRUNCATION_MARKER);
    status
}
