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

/// Read the actual Agent selection plus project-aware profile choices. This
/// uses no provider calls and leaves turn/cancellation state untouched.
pub fn models_value(agent: &Agent) -> Result<Value, AgentError> {
    let profiles = crate::config::model_catalog_in_workspace(
        agent.project_overrides().profile.as_deref(),
        agent.attachment_workspace_root(),
    )
    .map_err(|error| AgentError::InvalidTurn(error.to_string()))?;
    let status = agent.model_status()?;
    Ok(
        json!({ "command": "models", "route": "local", "accepted": true,
        "models": profiles, "selection": status }),
    )
}

/// Resource controls share the Agent owner used by provider turns. Hosts with
/// a worker queue must execute this there so cancellation stays responsive.
pub fn resource_control_value(
    agent: &mut Agent,
    input: &str,
    cancelled: impl Fn() -> bool,
) -> Result<Option<Value>, AgentError> {
    if let Some(request) =
        output_control_request(agent.session().session_id(), "tui-output-control", input)?
    {
        let mut value = agent
            .tool_output_control(request, &cancelled)
            .map_err(|e| AgentError::InvalidTurn(format!("{}: {e}", e.code())))?;
        value["command"] = json!("tool_output");
        return Ok(Some(value));
    }
    if let Some(request) =
        tree_control_request(agent.session().session_id(), "tui-tree-control", input)?
    {
        let select = matches!(request.tree, crate::protocol::TreeAction::Select { .. });
        let mut value = agent.tree_request(request, &cancelled)?;
        value["command"] = json!("tree");
        value["tree_selection"] = json!(select);
        return Ok(Some(value));
    }
    if input.trim() == "/compact" {
        let report = agent.compact_context_with_control(cancelled)?;
        let mut data =
            serde_json::to_value(report).map_err(|e| AgentError::InvalidTurn(e.to_string()))?;
        let object = data.as_object_mut().expect("compaction report object");
        object.insert("command".into(), json!("compact"));
        object.insert("route".into(), json!("local"));
        object.insert("accepted".into(), json!(true));
        object.insert("durable".into(), json!(true));
        object.insert(
            "next_sequence".into(),
            json!(agent.session().next_sequence()),
        );
        return Ok(Some(data));
    }
    let Some(command) = crate::slash::resource_control(input) else {
        return Ok(None);
    };
    if cancelled() {
        return Err(crate::backend::BackendError::Cancelled.into());
    }
    let data = if command == "reload" {
        let selected = input.trim().strip_prefix("/reload").unwrap_or("").trim();
        if selected.is_empty() {
            agent.reload_resources(cancelled)?
        } else {
            let workspace = agent.attachment_workspace_root().ok_or_else(|| {
                AgentError::InvalidTurn("resource selection requires a workspace".into())
            })?;
            let relative = Path::new(selected);
            if selected.len() > 4096
                || relative.is_absolute()
                || relative
                    .components()
                    .any(|component| matches!(component, Component::ParentDir))
            {
                return Err(AgentError::InvalidTurn(
                    "resource selection file must stay inside the workspace".into(),
                ));
            }
            let text = crate::skills::read_skill_text(&workspace.join(relative), &cancelled)
                .map_err(|error| {
                    if matches!(error, crate::skills::SkillError::Cancelled) {
                        AgentError::Backend(crate::backend::BackendError::Cancelled)
                    } else {
                        AgentError::InvalidTurn(error.to_string())
                    }
                })?;
            let mut paths: crate::resource_loader::ResourcePaths = serde_json::from_str(&text)
                .map_err(|error| {
                    AgentError::InvalidTurn(format!("resource selection JSON: {error}"))
                })?;
            for path in [
                &mut paths.user_skills,
                &mut paths.project_skills,
                &mut paths.user_templates,
                &mut paths.project_templates,
            ]
            .into_iter()
            .chain(paths.skill_paths.iter_mut())
            .chain(paths.template_paths.iter_mut())
            .chain(paths.text_resources.iter_mut().map(|value| &mut value.path))
            {
                if path.is_relative() {
                    *path = workspace.join(&*path);
                }
            }
            agent.configure_resources(paths, cancelled)?
        }
    } else {
        agent.resource_status()
    };
    Ok(Some(
        json!({"command": command, "route": "local", "accepted": true, "resources": data}),
    ))
}

/// Validate a dynamic prompt name before exposing it as a provider candidate
/// in an interactive host. Admission rereads selected skill bytes and checks
/// the captured hash; this classification never loads a body.
pub fn route_input_for_agent(
    agent: &Agent,
    input: &str,
) -> Result<crate::slash::InputRoute, crate::slash::SlashError> {
    if crate::slash::resource_input_candidate(input) {
        let name = input[1..].split_whitespace().next().unwrap_or("");
        let snapshot = agent.resource_snapshot();
        if name.strip_prefix("skill:").is_some_and(|name| {
            snapshot
                .skills
                .metadata()
                .any(|metadata| metadata.name == name)
        }) || snapshot.templates.get(name).is_some()
        {
            return Ok(crate::slash::InputRoute::Prompt(input.to_owned()));
        }
    }
    crate::slash::route_input(input)
}
/// `git status` is metadata only, but it can still contain one line per file.
pub const MAX_STATUS_BYTES: usize = 32 * 1024;
const MAX_UNTRACKED_DIFF_FILES: usize = 32;
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
    let requested_path = path.unwrap_or(".").to_owned();
    let relative = match path {
        Some(raw) => Some(resolve_relative(&workspace, raw, true)?),
        None => None,
    };

    // Git's binary marker is not a reviewable text patch. Surface an explicit
    // bounded binary result so approval/rendering cannot mistake it for text.
    if let Some(target) = relative.as_deref()
        && target.is_file()
        && file_looks_binary(target)?
    {
        let relative_display = target
            .strip_prefix(&workspace)
            .map(path_display)
            .unwrap_or_else(|_| path_display(target));
        return Ok(json!({
            "command": "diff",
            "source": "git",
            "route": "local",
            "accepted": true,
            "path": relative_display,
            "changed": true,
            "binary": true,
            "diff": "",
            "truncated": false,
            "status": bound_status(crate::security::redact_text(
                &git_status(&workspace, Some(target))?.text,
                &[],
            )),
        }));
    }

    let status = git_status(&workspace, relative.as_deref())?;
    if let Some(target) = relative.as_deref()
        && !target.exists()
        && status.text.trim().is_empty()
    {
        return Err(SlashActionError::PathDenied(requested_path));
    }
    let untracked = relative.as_deref().is_some_and(|target| {
        target.is_file() && status.untracked.iter().any(|path| path == target)
    });

    let (mut diff, mut exit_status, mut process_truncated) = if untracked {
        let target = relative
            .as_deref()
            .ok_or_else(|| SlashActionError::PathDenied("missing diff path".into()))?;
        let target_name = Path::new(".").join(
            target
                .strip_prefix(&workspace)
                .map_err(|_| SlashActionError::PathDenied(path_display(target)))?,
        );
        git_diff_process(
            &workspace,
            &[
                "diff",
                "--no-index",
                "--no-ext-diff",
                "--no-textconv",
                "--no-color",
                "--",
                "/dev/null",
            ],
            Some(&target_name),
        )?
    } else {
        let git_path = relative
            .as_deref()
            .map(|relative| {
                let path = relative
                    .strip_prefix(&workspace)
                    .map_err(|_| SlashActionError::PathDenied(path_display(relative)))?;
                // `Path::strip_prefix` returns an empty path for `/diff .`,
                // but Git treats an empty pathspec as invalid.
                Ok::<&Path, SlashActionError>(if path.as_os_str().is_empty() {
                    Path::new(".")
                } else {
                    path
                })
            })
            .transpose()?;
        let args = [
            "diff",
            "HEAD",
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            "--unified=3",
            "--",
        ];
        // Git receives the native path, never a normalized display string.
        // The preceding `--` also keeps path bytes out of option parsing.
        let first = git_diff_process(&workspace, &args, git_path)?;
        if first.1 == 128 && !first.2 {
            // A repository without a commit has no `HEAD`. Include both the
            // index and worktree diffs in that case instead of turning a
            // perfectly usable new checkout into a hard command error.
            let unstaged_args = [
                "diff",
                "--no-ext-diff",
                "--no-textconv",
                "--no-color",
                "--unified=3",
                "--",
            ];
            let unstaged = git_diff_process(&workspace, &unstaged_args, git_path)?;
            if unstaged.1 == 0 && !unstaged.2 && unstaged.0.is_empty() {
                let cached_args = [
                    "diff",
                    "--cached",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--no-color",
                    "--unified=3",
                    "--",
                ];
                git_diff_process(&workspace, &cached_args, git_path)?
            } else {
                unstaged
            }
        } else {
            first
        }
    };

    // A repository-wide (or directory-wide) review should not silently omit
    // untracked files. Git's normal `diff HEAD` intentionally ignores them,
    // so add a bounded synthetic `/dev/null` section for a small prefix of
    // ordinary untracked files. Explicit file reviews use the fast branch
    // above and are not duplicated here.
    if !untracked && relative.as_deref().is_none_or(|target| target.is_dir()) && !process_truncated
    {
        for target in &status.untracked {
            let target_name = Path::new(".").join(
                target
                    .strip_prefix(&workspace)
                    .map_err(|_| SlashActionError::PathDenied(path_display(target)))?,
            );
            let (section, section_status, section_truncated) = git_diff_process(
                &workspace,
                &[
                    "diff",
                    "--no-index",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--no-color",
                    "--",
                    "/dev/null",
                ],
                Some(&target_name),
            )?;
            if section_status != 1 && !section_truncated {
                return Err(SlashActionError::Git(format!(
                    "git diff exited with status {section_status}"
                )));
            }
            if !diff.is_empty() {
                diff.push('\n');
            }
            diff.push_str(&section);
            if section_truncated || diff.len() > MAX_SLASH_DIFF_BYTES {
                let (bounded, _) = bound_text(diff);
                diff = bounded;
                process_truncated = true;
                break;
            }
            exit_status = 0;
        }
    }

    // `git diff --no-index` exits 1 when differences exist.  All other
    // non-zero statuses are genuine failures and should be visible to the
    // owner rather than presented as an empty successful review.
    if exit_status != 0 && !process_truncated && !(untracked && exit_status == 1) {
        return Err(SlashActionError::Git(format!(
            "git diff exited with status {exit_status}"
        )));
    }
    let (diff, bounded_truncated) = bound_text(crate::security::redact_text(&diff, &[]));
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
        "status": bound_status(crate::security::redact_text(&status.text, &[])),
    }))
}

fn file_looks_binary(path: &Path) -> Result<bool, SlashActionError> {
    let file = fs::File::open(path).map_err(SlashActionError::Read)?;
    let mut bytes = Vec::new();
    file.take(8 * 1024)
        .read_to_end(&mut bytes)
        .map_err(SlashActionError::Read)?;
    Ok(std::str::from_utf8(&bytes).is_err() || bytes.contains(&0))
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
    // This reference is reopened for hashing, staging, and turn admission.
    // Keep the native path spelling: display normalization can turn a literal
    // Unix backslash into a separator and select a different workspace file.
    // Reject an unrepresentable path instead of creating a lossy I/O reference.
    let relative = relative
        .to_str()
        .ok_or_else(|| SlashActionError::PathDenied(path.to_owned()))?
        .to_owned();
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
    let display = path.to_string_lossy();
    if cfg!(windows) {
        display.replace('\\', "/")
    } else {
        // A Unix backslash is part of the filename, not a separator.
        display.into_owned()
    }
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

struct GitStatus {
    text: String,
    untracked: Vec<PathBuf>,
}

fn git_status(workspace: &Path, path: Option<&Path>) -> Result<GitStatus, SlashActionError> {
    let mut command = Command::new("git");
    command
        .current_dir(workspace)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_PAGER", "cat")
        .env("GIT_TERMINAL_PROMPT", "0")
        // The owner selects literal paths, independently of inherited Git
        // glob/case modes, which Git rejects when combined with literal mode.
        .env_remove("GIT_GLOB_PATHSPECS")
        .env_remove("GIT_ICASE_PATHSPECS")
        // An explicit workspace filename is not a glob or pathspec directive.
        .arg("--literal-pathspecs")
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
    Ok(GitStatus {
        // Preserve the user-facing porcelain text, including Git's quoting.
        // Lossy display bytes must never become a filesystem reference.
        untracked: untracked_paths(workspace, &bytes[..bytes.len().min(MAX_STATUS_BYTES)], path),
        text: bound_status(String::from_utf8_lossy(&bytes).into_owned()),
    })
}

fn untracked_paths(workspace: &Path, status: &[u8], scope: Option<&Path>) -> Vec<PathBuf> {
    status
        .split_inclusive(|byte| *byte == b'\n')
        .filter_map(|line| {
            // Only complete untracked records are candidates. A truncated
            // final path must not select another existing file with its prefix.
            // Rename/copy records have a different status and remain display only.
            let line = line.strip_suffix(b"\n")?;
            let raw = git_status_path(line.strip_prefix(b"?? ")?)?;
            let target = resolve_relative(workspace, &raw, false).ok()?;
            if !target.is_file() {
                return None;
            }
            if let Some(scope) = scope
                && !target.starts_with(scope)
            {
                return None;
            }
            Some(target)
        })
        .take(MAX_UNTRACKED_DIFF_FILES)
        .collect()
}

/// Decode Git's porcelain-v1 C-quoted filename, before any display conversion.
/// Git uses named ASCII escapes and three-digit octal byte escapes; ordinary
/// UTF-8 bytes may also appear when core.quotePath=false. The public path API
/// is UTF-8: reject unrepresentable bytes instead of choosing a lossy neighbor.
fn git_status_path(raw: &[u8]) -> Option<String> {
    if !raw.starts_with(b"\"") {
        return String::from_utf8(raw.to_vec()).ok();
    }
    let quoted = raw.strip_prefix(b"\"")?.strip_suffix(b"\"")?;
    let mut bytes = quoted.iter().copied();
    let mut decoded = Vec::with_capacity(quoted.len());
    while let Some(byte) = bytes.next() {
        decoded.push(match byte {
            b'"' => return None,
            b'\\' => match bytes.next()? {
                b'"' => b'"',
                b'\\' => b'\\',
                b'a' => 7,
                b'b' => 8,
                b't' => b'\t',
                b'n' => b'\n',
                b'v' => 11,
                b'f' => 12,
                b'r' => b'\r',
                first @ b'0'..=b'3' => {
                    let second = bytes.next()?;
                    let third = bytes.next()?;
                    if !(b'0'..=b'7').contains(&second) || !(b'0'..=b'7').contains(&third) {
                        return None;
                    }
                    (first - b'0') * 64 + (second - b'0') * 8 + (third - b'0')
                }
                _ => return None,
            },
            byte => byte,
        });
    }
    String::from_utf8(decoded).ok()
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
        // The owner selects literal paths, independently of inherited Git
        // glob/case modes, which Git rejects when combined with literal mode.
        .env_remove("GIT_GLOB_PATHSPECS")
        .env_remove("GIT_ICASE_PATHSPECS")
        .arg("--literal-pathspecs")
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

/// Build the same typed queue request used by JSONL and TUI tickets. This only
/// parses; the Agent/port owner is responsible for durable acknowledgement.
pub fn input_queue_control_request(
    session_id: &str,
    id: &str,
    input: &str,
) -> Result<Option<crate::protocol::InputQueueRequest>, AgentError> {
    let Some(action) = crate::slash::input_queue_control(input) else {
        return Ok(None);
    };
    let request = crate::protocol::InputQueueRequest {
        schema_version: crate::protocol::PROTOCOL_VERSION,
        id: id.into(),
        kind: "input_queue".into(),
        session_id: session_id.into(),
        input_queue: action.map_err(AgentError::InvalidTurn)?,
    };
    request
        .validate()
        .map_err(|e| AgentError::InvalidTurn(e.to_string()))?;
    Ok(Some(request))
}

/// Bind the shared grammar to the active session, never an implicit new owner.
pub fn tree_control_request(
    session_id: &str,
    id: &str,
    input: &str,
) -> Result<Option<crate::protocol::TreeRequest>, AgentError> {
    let Some(action) = crate::slash::tree_control(input) else {
        return Ok(None);
    };
    let request = crate::protocol::TreeRequest {
        schema_version: crate::protocol::PROTOCOL_VERSION,
        id: id.into(),
        kind: "tree".into(),
        session_id: session_id.into(),
        tree: action.map_err(AgentError::InvalidTurn)?,
    };
    request
        .validate()
        .map_err(|e| AgentError::InvalidTurn(e.to_string()))?;
    Ok(Some(request))
}

pub fn output_control_request(
    session_id: &str,
    id: &str,
    input: &str,
) -> Result<Option<crate::protocol::OutputRequest>, AgentError> {
    let Some(action) = crate::slash::output_control(input) else {
        return Ok(None);
    };
    let request = crate::protocol::OutputRequest {
        schema_version: crate::protocol::PROTOCOL_VERSION,
        id: id.into(),
        kind: "tool_output".into(),
        session_id: session_id.into(),
        output: action.map_err(AgentError::InvalidTurn)?,
    };
    request
        .validate()
        .map_err(|e| AgentError::InvalidTurn(e.to_string()))?;
    Ok(Some(request))
}

#[cfg(test)]
mod attachment_identity_tests {
    use super::*;
    #[cfg(unix)]
    use crate::{core::TurnInputRequest, session::SessionStore, tools::ToolContext};

    #[test]
    fn normalized_relative_attachment_remains_a_native_path() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        fs::create_dir(root.join("nested")).unwrap();
        let expected = Path::new("nested").join("文件 name.txt");
        fs::write(root.join(&expected), b"selected file").unwrap();
        let attachment = attachment_for_path(root, "./nested/./文件 name.txt").unwrap();
        let encoded = serde_json::to_string(&attachment).unwrap();
        let decoded: InputAttachment = serde_json::from_str(&encoded).unwrap();
        let path = decoded.path.as_deref().unwrap();
        assert_eq!(Path::new(path), expected);
        assert_eq!(fs::read(root.join(path)).unwrap(), b"selected file");
        assert_eq!(attachment.kind, AttachmentKind::File);
        assert_eq!(attachment.mime_type, "text/plain");
    }

    #[test]
    fn attachment_path_and_file_limits_remain_enforced() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        fs::write(root.join("valid.txt"), b"valid").unwrap();
        for denied in ["", " ", "../valid.txt", "valid.txt\n", ".", "missing.txt"] {
            assert!(attachment_for_path(root, denied).is_err(), "{denied:?}");
        }
        assert!(attachment_for_path(root, &"a".repeat(MAX_WORKSPACE_PATH_BYTES + 1)).is_err());
        assert!(attachment_for_path(root, root.join("valid.txt").to_str().unwrap()).is_err());
        let large = fs::File::create(root.join("large.bin")).unwrap();
        large.set_len(MAX_ATTACHMENT_BYTES as u64).unwrap();
        assert!(attachment_for_path(root, "large.bin").is_ok());
        large.set_len(MAX_ATTACHMENT_BYTES as u64 + 1).unwrap();
        assert!(matches!(
            attachment_for_path(root, "large.bin"),
            Err(SlashActionError::FileTooLarge { .. })
        ));
        assert!(matches!(
            file_digest(&canonical_workspace(root).unwrap(), "large.bin"),
            Err(SlashActionError::FileTooLarge { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn literal_backslash_attachment_needs_no_directory_counterpart() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let path = r#"only\quoted"name.txt"#;
        fs::write(root.join(path), b"literal only").unwrap();
        let session = SessionStore::open(root.join("session.jsonl")).unwrap();
        let mut agent = Agent::with_echo(session);
        agent.set_attachment_workspace(ToolContext::new(root).unwrap());
        let value = attach_value_at(&mut agent, root, path).unwrap();
        assert_eq!(value["path"], path);
        let encoded = serde_json::to_string(&value).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&encoded).unwrap(), value);
        assert_eq!(agent.pending_attachments()[0].path.as_deref(), Some(path));
    }

    #[cfg(unix)]
    #[test]
    fn attachment_symlink_components_remain_denied() {
        use std::os::unix::fs::symlink;
        let directory = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let root = directory.path();
        fs::create_dir(root.join("nested")).unwrap();
        fs::write(root.join("nested/file.txt"), b"inside").unwrap();
        fs::write(outside.path().join("file.txt"), b"outside").unwrap();
        symlink(root.join("nested/file.txt"), root.join(r"linked\file.txt")).unwrap();
        symlink(root.join("nested"), root.join("linked-dir")).unwrap();
        symlink(outside.path(), root.join("outside-dir")).unwrap();
        let session = SessionStore::open(root.join("session.jsonl")).unwrap();
        let mut agent = Agent::with_echo(session);
        agent.set_attachment_workspace(ToolContext::new(root).unwrap());
        for denied in [
            r"linked\file.txt",
            "linked-dir/file.txt",
            "outside-dir/file.txt",
        ] {
            assert!(matches!(
                attach_value_at(&mut agent, root, denied),
                Err(SlashActionError::PathDenied(_))
            ));
            assert!(agent.pending_attachments().is_empty());
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_backslash_still_separates_path_components() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        fs::create_dir(root.join("nested")).unwrap();
        fs::write(root.join(r"nested\file.txt"), b"windows native path").unwrap();
        let attachment = attachment_for_path(root, r"nested\file.txt").unwrap();
        let path = attachment.path.as_deref().unwrap();
        assert_eq!(Path::new(path), Path::new("nested/file.txt"));
        assert_eq!(fs::read(root.join(path)).unwrap(), b"windows native path");
        assert!(attachment_for_path(root, r"nested\..\file.txt").is_err());
        assert!(attachment_for_path(root, r"C:\outside.txt").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn literal_backslash_and_directory_attachment_keep_distinct_identities() {
        use std::os::unix::fs::MetadataExt;
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let literal = r"note\report.txt";
        let nested = "note/report.txt";
        let literal_bytes = b"literal backslash payload\n";
        let nested_bytes = b"directory payload with a different size\n";
        fs::create_dir(root.join("note")).unwrap();
        fs::write(root.join(literal), literal_bytes).unwrap();
        fs::write(root.join(nested), nested_bytes).unwrap();
        assert_ne!(
            fs::metadata(root.join(literal)).unwrap().ino(),
            fs::metadata(root.join(nested)).unwrap().ino()
        );
        for (index, (path, expected)) in [
            (literal, literal_bytes.as_slice()),
            (nested, nested_bytes.as_slice()),
        ]
        .into_iter()
        .enumerate()
        {
            let session = SessionStore::open(root.join(format!("session-{index}.jsonl"))).unwrap();
            let mut agent = Agent::with_echo(session);
            agent.set_attachment_workspace(ToolContext::new(root).unwrap());
            let value = attach_value_at(&mut agent, root, path).unwrap();
            let staged = agent.pending_attachments()[0].clone();
            let (_, staged_bytes) = ToolContext::new(root)
                .unwrap()
                .read_attachment(staged.path.as_deref().unwrap(), MAX_ATTACHMENT_BYTES)
                .unwrap();
            agent
                .submit(TurnInputRequest::new("inspect attachment"))
                .unwrap();
            let metadata = &agent.history()[0].metadata.as_ref().unwrap()["attachments"][0];
            println!(
                "requested={path:?} receipt={value} staged={staged:?} materialized={metadata}"
            );
            assert_eq!(value["path"], path);
            assert_eq!(staged.path.as_deref(), Some(path));
            assert_eq!(staged_bytes, expected);
            assert_eq!(value["size_bytes"], expected.len());
            assert_eq!(value["sha256"], format!("{:x}", Sha256::digest(expected)));
            assert_eq!(metadata["path"], path);
            assert_eq!(metadata["sha256"], value["sha256"]);
            assert_eq!(metadata["size_bytes"], value["size_bytes"]);
            let encoded = serde_json::to_string(&value).unwrap();
            let decoded: Value = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded["path"], path);
            if path == literal {
                assert!(encoded.contains(r"note\\report.txt"));
            }
            let journal = fs::read_to_string(agent.session().path()).unwrap();
            assert!(!journal.contains(std::str::from_utf8(expected).unwrap().trim()));
        }
    }
}

#[cfg(all(test, unix))]
mod diff_identity_tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;

    fn git(root: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(root)
            // Fixture setup must succeed even when the API under test inherits
            // conflicting modes from its independently spawned test process.
            .env_remove("GIT_GLOB_PATHSPECS")
            .env_remove("GIT_ICASE_PATHSPECS")
            .arg("--literal-pathspecs")
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    fn repository() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        git(root.path(), &["init", "-q"]);
        git(
            root.path(),
            &["config", "user.name", "diff identity fixture"],
        );
        git(root.path(), &["config", "user.email", "diff@example.test"]);
        git(root.path(), &["commit", "--allow-empty", "-qm", "initial"]);
        root
    }

    fn collision(tracked: bool) {
        let directory = repository();
        let root = directory.path();
        let literal = r"note\report.txt";
        let nested = "note/report.txt";
        fs::create_dir(root.join("note")).unwrap();
        fs::write(root.join(literal), "literal before\n").unwrap();
        fs::write(root.join(nested), "directory before\n").unwrap();
        assert_ne!(
            fs::metadata(root.join(literal)).unwrap().ino(),
            fs::metadata(root.join(nested)).unwrap().ino()
        );
        if tracked {
            git(root, &["add", "--", literal, nested]);
            git(root, &["commit", "-qm", "two different files"]);
        }
        fs::write(root.join(literal), "EXPECTED_LITERAL_CONTENT\n").unwrap();
        fs::write(root.join(nested), "WRONG_DIRECTORY_CONTENT\n").unwrap();
        let parsed = crate::slash::parse(r"/diff note\\report.txt")
            .unwrap()
            .unwrap();
        let crate::slash::SlashCommand::Diff { path } = parsed else {
            panic!("expected diff")
        };
        assert_eq!(path.as_deref(), Some(literal));
        let result = diff_value_at(root, path.as_deref());
        println!(
            "tracked={tracked} literal_inode={} nested_inode={} result={result:?}",
            fs::metadata(root.join(literal)).unwrap().ino(),
            fs::metadata(root.join(nested)).unwrap().ino()
        );
        let value = result.unwrap();
        let diff = value["diff"].as_str().unwrap();
        assert!(diff.contains("+EXPECTED_LITERAL_CONTENT"), "{diff}");
        assert!(!diff.contains("WRONG_DIRECTORY_CONTENT"), "{diff}");
        assert_eq!(value["path"], literal);
        let other = diff_value_at(root, Some(nested)).unwrap();
        assert!(
            other["diff"]
                .as_str()
                .unwrap()
                .contains("+WRONG_DIRECTORY_CONTENT")
        );
        assert!(
            !other["diff"]
                .as_str()
                .unwrap()
                .contains("EXPECTED_LITERAL_CONTENT")
        );
    }

    #[test]
    fn tracked_backslash_diff_reads_only_selected_file() {
        collision(true);
    }

    #[test]
    fn inherited_git_pathspec_modes_do_not_break_literal_diff() {
        for variable in ["GIT_GLOB_PATHSPECS", "GIT_ICASE_PATHSPECS"] {
            // Set only the child's environment. Never mutate the process-wide
            // environment shared by other concurrently executing Rust tests.
            let output = Command::new(std::env::current_exe().unwrap())
                .env(variable, "1")
                .args([
                    "--exact",
                    "slash_actions::diff_identity_tests::tracked_backslash_diff_reads_only_selected_file",
                    "--nocapture",
                ])
                .output()
                .unwrap();
            println!(
                "{variable}=1 child={}\n{}\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(output.status.success(), "{variable} broke literal diff");
            assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        }
    }

    #[test]
    fn untracked_backslash_diff_reads_only_selected_file() {
        collision(false);
    }

    fn literal_pathspec(selected: &str, neighbor: &str) {
        let directory = repository();
        let root = directory.path();
        fs::write(root.join(selected), "selected before\n").unwrap();
        fs::write(root.join(neighbor), "neighbor before\n").unwrap();
        git(root, &["add", "--", selected, neighbor]);
        git(root, &["commit", "-qm", "literal pathspec files"]);
        fs::write(root.join(selected), "EXPECTED_EXACT_PATH\n").unwrap();
        fs::write(root.join(neighbor), "WRONG_GLOB_NEIGHBOR\n").unwrap();
        let result = diff_value_at(root, Some(selected));
        println!("selected={selected:?} result={result:?}");
        let value = result.unwrap();
        let diff = value["diff"].as_str().unwrap();
        assert!(diff.contains("+EXPECTED_EXACT_PATH"), "{diff}");
        assert!(!diff.contains("WRONG_GLOB_NEIGHBOR"), "{diff}");
        assert!(!value["status"].as_str().unwrap().contains(neighbor));
    }

    #[test]
    fn bracket_filename_is_not_a_git_glob() {
        literal_pathspec("selected[ab].txt", "selecteda.txt");
    }

    #[test]
    fn magic_filename_is_not_a_git_pathspec_directive() {
        literal_pathspec(":(glob)*.txt", "neighbor.txt");
    }

    #[test]
    fn unborn_head_preserves_native_paths_in_cached_and_unstaged_fallbacks() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        git(root, &["init", "-q"]);
        let selected = r"new\file.txt";
        fs::create_dir(root.join("new")).unwrap();
        fs::write(root.join(selected), "EXPECTED_STAGED\n").unwrap();
        fs::write(root.join("new/file.txt"), "WRONG_STAGED_NEIGHBOR\n").unwrap();
        git(root, &["add", "--", selected, "new/file.txt"]);
        let staged = diff_value_at(root, Some(selected)).unwrap();
        assert!(
            staged["diff"]
                .as_str()
                .unwrap()
                .contains("+EXPECTED_STAGED")
        );
        assert!(
            !staged["diff"]
                .as_str()
                .unwrap()
                .contains("WRONG_STAGED_NEIGHBOR")
        );
        fs::write(root.join(selected), "EXPECTED_UNSTAGED\n").unwrap();
        let unstaged = diff_value_at(root, Some(selected)).unwrap();
        assert!(
            unstaged["diff"]
                .as_str()
                .unwrap()
                .contains("+EXPECTED_UNSTAGED")
        );
        assert!(
            !unstaged["diff"]
                .as_str()
                .unwrap()
                .contains("WRONG_STAGED_NEIGHBOR")
        );
    }

    #[test]
    fn deleted_backslash_file_does_not_select_existing_neighbor() {
        let directory = repository();
        let root = directory.path();
        let selected = r"deleted\file.txt";
        fs::create_dir(root.join("deleted")).unwrap();
        fs::write(root.join(selected), "EXPECTED_DELETED\n").unwrap();
        fs::write(root.join("deleted/file.txt"), "neighbor before\n").unwrap();
        git(root, &["add", "--", selected, "deleted/file.txt"]);
        git(root, &["commit", "-qm", "before deletion"]);
        fs::remove_file(root.join(selected)).unwrap();
        fs::write(root.join("deleted/file.txt"), "WRONG_EXISTING_NEIGHBOR\n").unwrap();
        let value = diff_value_at(root, Some(selected)).unwrap();
        assert!(
            value["diff"]
                .as_str()
                .unwrap()
                .contains("-EXPECTED_DELETED")
        );
        assert!(
            !value["diff"]
                .as_str()
                .unwrap()
                .contains("WRONG_EXISTING_NEIGHBOR")
        );
    }

    #[test]
    fn leading_option_filename_is_a_file_in_both_git_modes() {
        let directory = repository();
        let root = directory.path();
        let selected = "--output=not-an-option.txt";
        fs::write(root.join(selected), "EXPECTED_OPTION_FILENAME\n").unwrap();
        let value = diff_value_at(root, Some(selected)).unwrap();
        assert!(
            value["diff"]
                .as_str()
                .unwrap()
                .contains("+EXPECTED_OPTION_FILENAME")
        );
        git(root, &["add", "--", selected]);
        git(root, &["commit", "-qm", "option-looking filename"]);
        fs::write(root.join(selected), "EXPECTED_TRACKED_OPTION\n").unwrap();
        let value = diff_value_at(root, Some(selected)).unwrap();
        assert!(
            value["diff"]
                .as_str()
                .unwrap()
                .contains("+EXPECTED_TRACKED_OPTION")
        );
        assert!(!root.join("not-an-option.txt").exists());
    }

    #[test]
    fn literal_directory_scope_and_untracked_prefix_bound_remain_intact() {
        let directory = repository();
        let root = directory.path();
        for folder in ["scope[ab]", "scopea"] {
            fs::create_dir(root.join(folder)).unwrap();
            fs::write(root.join(folder).join("seed.txt"), "seed\n").unwrap();
        }
        git(root, &["add", "--", "scope[ab]", "scopea"]);
        git(root, &["commit", "-qm", "tracked directory seeds"]);
        fs::write(root.join("scope[ab]/new.txt"), "EXPECTED_DIRECTORY_SCOPE\n").unwrap();
        fs::write(root.join("scopea/new.txt"), "WRONG_DIRECTORY_SCOPE\n").unwrap();
        let value = diff_value_at(root, Some("scope[ab]")).unwrap();
        assert!(
            value["diff"]
                .as_str()
                .unwrap()
                .contains("+EXPECTED_DIRECTORY_SCOPE")
        );
        assert!(
            !value["diff"]
                .as_str()
                .unwrap()
                .contains("WRONG_DIRECTORY_SCOPE")
        );
        let bounded = repository();
        for index in 0..MAX_UNTRACKED_DIFF_FILES + 8 {
            fs::write(
                bounded.path().join(format!("file-{index:02}.txt")),
                "bounded\n",
            )
            .unwrap();
        }
        let value = diff_value_at(bounded.path(), None).unwrap();
        assert_eq!(
            value["diff"]
                .as_str()
                .unwrap()
                .matches("diff --git ")
                .count(),
            MAX_UNTRACKED_DIFF_FILES
        );
        assert!(value["diff"].as_str().unwrap().len() <= MAX_SLASH_DIFF_BYTES);
        assert!(value["status"].as_str().unwrap().len() <= MAX_STATUS_BYTES);
    }

    #[test]
    fn quoted_untracked_paths_are_reviewed_without_changing_status_text() {
        let directory = repository();
        let root = directory.path();
        for folder in ["scope[ab]", "scopea", "scope[ab]/note"] {
            fs::create_dir_all(root.join(folder)).unwrap();
            fs::write(root.join(folder).join("seed"), "seed\n").unwrap();
        }
        git(root, &["add", "."]);
        git(root, &["commit", "-qm", "directory seeds"]);
        let names = [
            r"note\file",
            "note/file",
            "中文 file",
            "a\"quote",
            "name[ab]",
            ":(glob)*",
        ];
        for (index, name) in names.iter().enumerate() {
            fs::write(
                root.join("scope[ab]").join(name),
                format!("QUOTED_FILE_{index}\n"),
            )
            .unwrap();
        }
        fs::write(root.join("scopea/other"), "OUTSIDE_QUOTED_SCOPE\n").unwrap();
        assert_ne!(
            fs::metadata(root.join(r"scope[ab]/note\file"))
                .unwrap()
                .ino(),
            fs::metadata(root.join("scope[ab]/note/file"))
                .unwrap()
                .ino()
        );
        for quote in ["true", "false"] {
            git(root, &["config", "core.quotePath", quote]);
            for scope in [Some("scope[ab]"), None, Some(".")] {
                let value = diff_value_at(root, scope).unwrap();
                let diff = value["diff"].as_str().unwrap();
                for index in 0..names.len() {
                    assert!(
                        diff.contains(&format!("+QUOTED_FILE_{index}")),
                        "{quote}/{scope:?}: {value}"
                    );
                }
                if scope == Some("scope[ab]") {
                    assert!(!diff.contains("OUTSIDE_QUOTED_SCOPE"));
                }
                assert_eq!(
                    value["status"],
                    git(
                        root,
                        &[
                            "status",
                            "--porcelain=v1",
                            "--untracked-files=normal",
                            "--",
                            scope.unwrap_or(".")
                        ]
                    )
                );
            }
        }
    }

    #[test]
    fn porcelain_path_decoding_covers_all_octal_bytes_without_lossy_identity() {
        for byte in 0_u8..=255 {
            let quoted = format!("\"\\{byte:03o}\"");
            assert_eq!(
                git_status_path(quoted.as_bytes()),
                String::from_utf8(vec![byte]).ok()
            );
        }
        assert_eq!(git_status_path(br#""a\\b\"c""#).as_deref(), Some("a\\b\"c"));
        for invalid in [
            br#""unterminated"#.as_slice(),
            br#""bad\x""#,
            br#""bad\77""#,
            br#""bad\400""#,
            br#""bad"quote""#,
        ] {
            assert!(git_status_path(invalid).is_none(), "{invalid:?}");
        }
        let directory = tempfile::tempdir().unwrap();
        let root = canonical_workspace(directory.path()).unwrap();
        fs::write(root.join("prefix"), "must not be selected by truncation").unwrap();
        fs::write(
            root.join("bad�"),
            "must not be selected by lossy conversion",
        )
        .unwrap();
        for status in [
            b"?? prefix".as_slice(),
            b"?? bad\xff\n",
            b"R  other -> prefix\n",
            b"C  other -> prefix\n",
        ] {
            assert!(
                untracked_paths(&root, status, None).is_empty(),
                "{status:?}"
            );
        }
        assert_eq!(
            untracked_paths(&root, b"?? prefix\n?? incomplete", None),
            vec![root.join("prefix")]
        );
    }

    #[test]
    fn decoded_untracked_paths_keep_control_and_symlink_restrictions() {
        use std::os::unix::fs::symlink;
        let directory = tempfile::tempdir().unwrap();
        let root = canonical_workspace(directory.path()).unwrap();
        fs::write(root.join("ordinary"), "regular").unwrap();
        fs::write(root.join("line\nname"), "denied control").unwrap();
        symlink(root.join("ordinary"), root.join("link")).unwrap();
        let status = b"?? \"line\\nname\"\n?? link\n?? ../ordinary\n?? ordinary\n";
        assert_eq!(
            untracked_paths(&root, status, None),
            vec![root.join("ordinary")]
        );
    }
}
