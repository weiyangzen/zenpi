//! Explicit advanced search backed by an already-installed ripgrep. No tool
//! download, shell-language execution, persistent index or extra service.
use crate::tools::{SupervisedChild, ToolError};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const MAX_PIPE_BYTES: usize = 256 * 1024;
const MAX_STDERR_BYTES: usize = 8192;
const MAX_FILES: usize = 256;
const FILES_PER_CHILD: usize = 16;
pub const MAX_SEARCH_CHUNKS: usize = MAX_FILES.div_ceil(FILES_PER_CHILD);
const MAX_FILE_BYTES: u64 = 1024 * 1024;
const MAX_DEPTH: usize = 64;
const MAX_LINE_BYTES: usize = 2048;
const MAX_DURATION: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub struct SearchBackend {
    executable: Option<PathBuf>,
}
impl SearchBackend {
    pub fn discover() -> Self {
        let executable = std::env::var_os("PATH")
            .into_iter()
            .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
            .filter(|dir| dir.is_absolute())
            .map(|dir| dir.join("rg"))
            .find_map(|path| {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if fs::metadata(&path)
                        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
                    {
                        return path.canonicalize().ok();
                    }
                }
                None
            });
        Self { executable }
    }
    /// Explicit host configuration, also used to test unavailable capability.
    /// Never populate this path from model tool arguments.
    pub fn configured(executable: Option<PathBuf>) -> Result<Self, ToolError> {
        if executable.as_ref().is_some_and(|path| !path.is_absolute()) {
            return Err(ToolError::InvalidArguments(
                "search executable must be host-owned absolute path".into(),
            ));
        }
        Ok(Self { executable })
    }
    pub fn executable_path(&self) -> Option<&Path> {
        self.executable.as_deref()
    }

    pub fn available(&self) -> bool {
        self.executable.is_some()
    }

    pub fn search(
        &self,
        workspace: &Path,
        target: &Path,
        options: &SearchOptions,
        cancel: &dyn Fn() -> bool,
    ) -> Result<Value, ToolError> {
        options.validate()?;
        let deadline = Instant::now() + MAX_DURATION;
        let mut reasons = BTreeSet::new();
        // Validate regex even when an empty directory has no eligible files.
        if options.regex {
            self.run(
                workspace,
                &[
                    "--json".into(),
                    "-e".into(),
                    options.query.clone(),
                    "-".into(),
                ],
                deadline,
                cancel,
            )?;
        }
        let files = self.files(
            workspace,
            target,
            options.glob.as_deref(),
            options.ignore,
            options.hidden,
            true,
            deadline,
            cancel,
            &mut reasons,
        )?;
        let mut matches: Vec<Value> = Vec::new();
        let mut context: Vec<Value> = Vec::new();
        let mut searched = 0;
        for batch in files.chunks(FILES_PER_CHILD) {
            if matches.len() >= options.max_matches {
                reasons.insert("match_limit");
                break;
            }
            let mut args = vec![
                "--json".into(),
                "--line-number".into(),
                "--max-filesize".into(),
                MAX_FILE_BYTES.to_string(),
                "--context".into(),
                options.context.to_string(),
            ];
            if !options.regex {
                args.push("--fixed-strings".into());
            }
            if !options.case_sensitive {
                args.push("--ignore-case".into());
            }
            args.extend(["-e".into(), options.query.clone(), "--".into()]);
            args.extend(batch.iter().cloned());
            let output = self.run(workspace, &args, deadline, cancel)?;
            searched += batch.len();
            if output.truncated {
                reasons.insert("output_bytes");
            }
            for line in output.stdout.split(|b| *b == b'\n') {
                if line.is_empty() {
                    continue;
                }
                let event: Value = match serde_json::from_slice(line) {
                    Ok(event) => event,
                    Err(_) if output.truncated => {
                        reasons.insert("partial_event");
                        continue;
                    }
                    Err(_) => {
                        return Err(ToolError::InvalidArguments(
                            "invalid ripgrep JSON output".into(),
                        ));
                    }
                };
                let kind = event["type"].as_str().unwrap_or("");
                let binary_end = kind == "end" && event["data"]["binary_offset"].is_u64();
                if kind != "match" && kind != "context" && !binary_end {
                    continue;
                }
                let Some(path) = event["data"]["path"]["text"].as_str() else {
                    reasons.insert("non_utf8_path");
                    continue;
                };
                let path = checked_result_path(workspace, path)?;
                if crate::tool_output::private_output_path_inode(&workspace.join(&path))? {
                    return Err(ToolError::PathDenied(
                        "private output inode in search result".into(),
                    ));
                }
                if !batch.contains(&path) {
                    return Err(ToolError::PathDenied(
                        "search result outside admitted file batch".into(),
                    ));
                }
                if binary_end {
                    // rg can publish text matches before reporting the NUL
                    // offset in its end event. They are not a complete text file.
                    reasons.insert("binary_content");
                    matches.retain(|record| record["path"] != path);
                    context.retain(|record| record["path"] != path);
                    continue;
                }
                let Some(text) = event["data"]["lines"]["text"].as_str() else {
                    reasons.insert("non_utf8_content");
                    continue;
                };
                let Some(number) = event["data"]["line_number"].as_u64() else {
                    return Err(ToolError::InvalidArguments(
                        "ripgrep line number missing".into(),
                    ));
                };
                if text.contains('\0') {
                    reasons.insert("binary_content");
                    continue;
                }
                let text = text.trim_end_matches(['\r', '\n']);
                if text.len() > MAX_LINE_BYTES {
                    reasons.insert("line_bytes");
                }
                let record =
                    json!({"path":path,"line":number,"text":bounded_text(text,MAX_LINE_BYTES)});
                if kind == "match" {
                    if matches.len() < options.max_matches {
                        matches.push(record);
                    } else {
                        reasons.insert("match_limit");
                    }
                } else if context.len() < options.max_matches.saturating_mul(options.context * 2) {
                    context.push(record);
                } else {
                    reasons.insert("context_limit");
                }
            }
            // The pipe cap may have interrupted this batch. Do not pretend
            // later batches turn the resulting partial search into complete.
            if output.truncated {
                break;
            }
        }
        Ok(
            json!({"query":options.query,"matches":matches,"context":context,"files_visited":searched,
            "engine":"ripgrep","regex":options.regex,"ignore":options.ignore,"hidden":options.hidden,
            "max_depth":MAX_DEPTH,"max_file_bytes":MAX_FILE_BYTES,"file_limit":MAX_FILES,
            "truncated":!reasons.is_empty(),"truncation_reasons":reasons}),
        )
    }
    // Keep the bounded search options explicit at the existing host call site.
    #[allow(clippy::too_many_arguments)]
    pub fn find(
        &self,
        workspace: &Path,
        target: &Path,
        glob: &str,
        ignore: bool,
        hidden: bool,
        limit: usize,
        cancel: &dyn Fn() -> bool,
    ) -> Result<Value, ToolError> {
        validate_glob(glob)?;
        if !(1..=MAX_FILES).contains(&limit) {
            return Err(ToolError::InvalidArguments(
                "find limit must be 1..256".into(),
            ));
        }
        let mut reasons = BTreeSet::new();
        let mut files = self.files(
            workspace,
            target,
            Some(glob),
            ignore,
            hidden,
            false,
            Instant::now() + MAX_DURATION,
            cancel,
            &mut reasons,
        )?;
        if files.len() > limit {
            files.truncate(limit);
            reasons.insert("result_limit");
        }
        Ok(
            json!({"paths":files,"engine":"ripgrep","glob":glob,"ignore":ignore,"hidden":hidden,
            "max_depth":MAX_DEPTH,"truncated":!reasons.is_empty(),"truncation_reasons":reasons}),
        )
    }
    #[allow(clippy::too_many_arguments)]
    fn files(
        &self,
        workspace: &Path,
        target: &Path,
        glob: Option<&str>,
        ignore: bool,
        hidden: bool,
        content_search: bool,
        deadline: Instant,
        cancel: &dyn Fn() -> bool,
        reasons: &mut BTreeSet<&'static str>,
    ) -> Result<Vec<String>, ToolError> {
        if let Some(glob) = glob {
            validate_glob(glob)?;
        }
        if !target.starts_with(workspace) || crate::tools::private_output_path(target) {
            return Err(ToolError::PathDenied(
                "search root outside workspace".into(),
            ));
        }
        let target = target
            .strip_prefix(workspace)
            .map_err(|_| ToolError::PathDenied("search root outside workspace".into()))?;
        let target = if target.as_os_str().is_empty() {
            ".".into()
        } else {
            target
                .to_str()
                .ok_or_else(|| ToolError::InvalidArguments("non-UTF8 search root".into()))?
                .to_owned()
        };
        let list = |filter: Option<&str>, ignore: bool, max_depth: usize| {
            let mut args = vec![
                "--files".into(),
                "--null".into(),
                "--max-depth".into(),
                max_depth.to_string(),
                "--no-ignore-parent".into(),
                "--no-ignore-global".into(),
                "--no-ignore-exclude".into(),
            ];
            if !workspace.join(".git").exists() {
                args.push("--no-require-git".into());
            }
            if !ignore {
                args.push("--no-ignore".into());
            }
            if hidden {
                args.push("--hidden".into());
            }
            if let Some(glob) = filter {
                args.extend(["--glob".into(), glob.into()]);
            }
            args.extend([
                "--glob".into(),
                "!**/.git/**".into(),
                // Host-private output is never an ordinary search source,
                // even with hidden=true, ignore=false or a positive glob.
                "--iglob".into(),
                "!**/.zenpi-output-*/**".into(),
                "--".into(),
                target.clone(),
            ]);
            self.run(workspace, &args, deadline, cancel)
        };
        let first = list(None, ignore, MAX_DEPTH)?;
        if first.truncated {
            reasons.insert("path_listing_bytes");
        }
        let parse = |bytes: &[u8]| -> Result<BTreeSet<String>, ToolError> {
            bytes
                .split_inclusive(|b| *b == 0)
                .filter(|piece| piece.last() == Some(&0))
                .map(|piece| {
                    let path = std::str::from_utf8(&piece[..piece.len() - 1]).map_err(|_| {
                        ToolError::InvalidArguments("non-UTF8 search result path".into())
                    })?;
                    checked_result_path(workspace, path)
                })
                .collect()
        };
        let mut candidates = parse(&first.stdout)?;
        if let Some(glob) = glob {
            // A positive rg --glob overrides .gitignore. Intersect independent
            // listings so ignore=true remains authoritative even with a glob.
            let matched = list(Some(glob), false, MAX_DEPTH)?;
            if matched.truncated {
                reasons.insert("glob_listing_bytes");
            }
            let matched = parse(&matched.stdout)?;
            candidates.retain(|path| matched.contains(path));
        }
        // ripgrep does not emit a depth-limit diagnostic. Probe with a
        // wider, still bounded depth and compare the same ignore/hidden/glob
        // projection so deep files are reported as truncated while an empty
        // directory boundary remains complete.
        if candidates.is_empty()
            && !reasons.contains("path_listing_bytes")
            && !reasons.contains("glob_listing_bytes")
        {
            let wide_first = list(None, ignore, MAX_DEPTH.saturating_mul(4))?;
            let mut wide = parse(&wide_first.stdout)?;
            if let Some(glob) = glob {
                let wide_matched = list(Some(glob), false, MAX_DEPTH.saturating_mul(4))?;
                let wide_matched = parse(&wide_matched.stdout)?;
                wide.retain(|path| wide_matched.contains(path));
            }
            if wide.iter().any(|path| !candidates.contains(path)) {
                reasons.insert("depth_limit");
            }
        }
        let mut files = Vec::new();
        for candidate in candidates {
            if cancel() {
                return Err(ToolError::Cancelled);
            }
            let path = workspace.join(&candidate);
            let metadata = fs::symlink_metadata(&path)?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                reasons.insert("non_regular_file");
                continue;
            }
            if crate::tool_output::private_output_path_inode(&path)? {
                continue;
            }
            if content_search && metadata.len() > MAX_FILE_BYTES {
                reasons.insert("file_bytes");
                continue;
            }
            let canonical = path.canonicalize()?;
            if !canonical.starts_with(workspace) || crate::tools::private_output_path(&canonical) {
                return Err(ToolError::PathDenied(candidate));
            }
            if files.len() == MAX_FILES {
                reasons.insert("file_limit");
                break;
            }
            files.push(candidate);
        }
        Ok(files)
    }
    #[cfg(unix)]
    fn run(
        &self,
        workspace: &Path,
        args: &[String],
        deadline: Instant,
        cancel: &dyn Fn() -> bool,
    ) -> Result<SearchOutput, ToolError> {
        use std::os::unix::process::CommandExt;
        if cancel() {
            return Err(ToolError::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(ToolError::CommandTimeout(MAX_DURATION.as_millis() as u64));
        }
        let executable = self.executable.as_ref().ok_or_else(|| {
            ToolError::Unsupported(
                "ripgrep is not installed; default literal search remains available".into(),
            )
        })?;
        let mut command = Command::new(executable);
        command.args(["--no-config", "--color", "never", "--threads", "1"]);
        command
            .args(args)
            .current_dir(workspace)
            .env_clear()
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in crate::security::child_environment() {
            command.env(key, value);
        }
        let child = command.spawn().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ToolError::Unsupported("configured ripgrep executable is missing".into())
            } else {
                ToolError::Io(e)
            }
        })?;
        let mut child = SupervisedChild::new(child);
        let mut stdout = child
            .child
            .stdout
            .take()
            .ok_or_else(|| ToolError::CommandFailed("search stdout missing".into()))?;
        let mut stderr = child
            .child
            .stderr
            .take()
            .ok_or_else(|| ToolError::CommandFailed("search stderr missing".into()))?;
        crate::tools::make_pipe_nonblocking(&stdout)?;
        crate::tools::make_pipe_nonblocking(&stderr)?;
        let mut out = Vec::new();
        let mut err = Vec::new();
        let mut out_eof = false;
        let mut err_eof = false;
        let mut truncated = false;
        let status = loop {
            if cancel() {
                return Err(ToolError::Cancelled);
            }
            if Instant::now() >= deadline {
                return Err(ToolError::CommandTimeout(MAX_DURATION.as_millis() as u64));
            }
            let mut buffer = [0u8; 8192];
            for _ in 0..8 {
                match stdout.read(&mut buffer) {
                    Ok(0) => {
                        out_eof = true;
                        break;
                    }
                    Ok(n) => {
                        let keep = n.min(MAX_PIPE_BYTES.saturating_sub(out.len()));
                        out.extend_from_slice(&buffer[..keep]);
                        if keep < n {
                            truncated = true;
                            break;
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(e.into()),
                }
            }
            for _ in 0..8 {
                match stderr.read(&mut buffer) {
                    Ok(0) => {
                        err_eof = true;
                        break;
                    }
                    Ok(n) => {
                        let keep = n.min(MAX_STDERR_BYTES.saturating_sub(err.len()));
                        err.extend_from_slice(&buffer[..keep]);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(e.into()),
                }
            }
            if truncated {
                break None;
            }
            if let Some(status) = child.child.try_wait()?
                && out_eof
                && err_eof
            {
                break Some(status);
            }
            std::thread::sleep(Duration::from_millis(2));
        };
        child.cleanup();
        if !truncated && !status.is_some_and(|status| matches!(status.code(), Some(0 | 1))) {
            return Err(ToolError::InvalidArguments(format!(
                "ripgrep failed: {}",
                bounded_text(&String::from_utf8_lossy(&err), MAX_STDERR_BYTES)
            )));
        }
        Ok(SearchOutput {
            stdout: out,
            truncated,
        })
    }
    #[cfg(not(unix))]
    fn run(
        &self,
        _: &Path,
        _: &[String],
        _: Instant,
        _: &dyn Fn() -> bool,
    ) -> Result<SearchOutput, ToolError> {
        Err(ToolError::Unsupported(
            "search process containment is unavailable on this platform".into(),
        ))
    }
}
struct SearchOutput {
    stdout: Vec<u8>,
    truncated: bool,
}
#[derive(Debug)]
pub struct SearchOptions {
    pub query: String,
    pub regex: bool,
    pub glob: Option<String>,
    pub ignore: bool,
    pub hidden: bool,
    pub context: usize,
    pub case_sensitive: bool,
    pub max_matches: usize,
}
impl SearchOptions {
    fn validate(&self) -> Result<(), ToolError> {
        if self.query.is_empty()
            || self.query.len() > 4096
            || self.query.contains('\0')
            || self.context > 10
            || !(1..=100).contains(&self.max_matches)
        {
            return Err(ToolError::InvalidArguments(
                "advanced search arguments exceed limits".into(),
            ));
        }
        if let Some(glob) = &self.glob {
            validate_glob(glob)?;
        }
        Ok(())
    }
}
fn validate_glob(glob: &str) -> Result<(), ToolError> {
    if glob.is_empty() || glob.len() > 4096 || glob.contains(['\0', '\r', '\n']) {
        Err(ToolError::InvalidArguments(
            "glob must be nonempty and at most 4096 bytes".into(),
        ))
    } else {
        Ok(())
    }
}
fn checked_result_path(workspace: &Path, path: &str) -> Result<String, ToolError> {
    let path = Path::new(path);
    if crate::tools::private_output_path(path) {
        return Err(ToolError::PathDenied(path.display().to_string()));
    }
    let relative = if path.is_absolute() {
        path.strip_prefix(workspace)
            .map_err(|_| ToolError::PathDenied(path.display().to_string()))?
    } else {
        path
    };
    if relative.components().any(|part| {
        !matches!(
            part,
            std::path::Component::Normal(_) | std::path::Component::CurDir
        )
    }) {
        return Err(ToolError::PathDenied(path.display().to_string()));
    }
    let relative = relative
        .components()
        .filter(|part| *part != std::path::Component::CurDir)
        .collect::<PathBuf>();
    let text = relative
        .to_str()
        .ok_or_else(|| ToolError::InvalidArguments("non-UTF8 search path".into()))?;
    if text.len() > 4096 || text.contains(['\0', '\r', '\n']) {
        return Err(ToolError::PathDenied(text.into()));
    }
    Ok(text.into())
}
fn bounded_text(text: &str, max: usize) -> &str {
    let mut end = max.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}
