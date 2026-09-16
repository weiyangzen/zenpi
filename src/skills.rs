//! Bounded project/user skill manifests and lifecycle hooks.

use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const SKILL_MANIFEST: &str = "skill.toml";
pub const MAX_SKILL_INSTRUCTIONS_BYTES: usize = 64 * 1024;
pub const SKILL_MARKDOWN: &str = "SKILL.md";
pub const MAX_SKILL_FILE_BYTES: usize = 128 * 1024;
pub const MAX_SKILL_FILES: usize = 4096;
pub const MAX_SKILL_DEPTH: usize = 32;
const MAX_SKILL_INDEX_BYTES: usize = 1024 * 1024;

/// Provenance is explicit and independent of the process working directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillScope {
    User,
    Project,
    Explicit,
}

/// A discovery record never retains the Markdown body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    pub disable_model_invocation: bool,
    pub source: PathBuf,
    pub base_dir: PathBuf,
    pub scope: SkillScope,
    pub source_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillCollision {
    pub name: String,
    pub winner: PathBuf,
    pub loser: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillInvocation {
    Explicit,
    Model,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillBody {
    pub metadata: SkillMetadata,
    pub body: String,
    /// Literal caller data; substitution belongs to the prompt-template owner.
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillManifest {
    pub name: String,
    pub version: String,
    pub instructions: String,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub hooks: SkillHooks,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillHooks {
    #[serde(default)]
    pub prompt_prefix: Option<String>,
    #[serde(default)]
    pub context_prefix: Option<String>,
    #[serde(default)]
    pub tool_allowlist: Vec<String>,
    #[serde(default)]
    pub session_close: Option<String>,
}

impl SkillManifest {
    pub fn validate(&self) -> Result<(), SkillError> {
        validate_id(&self.name)?;
        if self.version.trim().is_empty()
            || self.version.len() > 64
            || self.version.chars().any(char::is_control)
        {
            return Err(SkillError::Invalid("version is invalid".into()));
        }
        if self.instructions.trim().is_empty()
            || self.instructions.len() > MAX_SKILL_INSTRUCTIONS_BYTES
            || self.instructions.contains('\0')
        {
            return Err(SkillError::Invalid(
                "instructions are invalid or too large".into(),
            ));
        }
        for tool in &self.tools {
            validate_id(tool)?;
        }
        for hook in [
            self.hooks.prompt_prefix.as_deref(),
            self.hooks.context_prefix.as_deref(),
            self.hooks.session_close.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            if hook.len() > 16 * 1024 || hook.contains('\0') {
                return Err(SkillError::Invalid(
                    "hook output is invalid or too large".into(),
                ));
            }
        }
        for tool in &self.hooks.tool_allowlist {
            validate_id(tool)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedSkill {
    pub manifest: SkillManifest,
    pub source: PathBuf,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SkillSet {
    skills: BTreeMap<String, LoadedSkill>,
    markdown: BTreeMap<String, SkillMetadata>,
    collisions: Vec<SkillCollision>,
}

impl SkillSet {
    /// Load user skills first and project skills second so the project-local
    /// manifest deliberately wins for an identical skill ID.
    pub fn load(user_root: &Path, project_root: &Path) -> Result<Self, SkillError> {
        Self::load_with_paths(user_root, project_root, &[], || false)
    }

    /// Build a complete replacement before publication. Priority is explicit
    /// paths (later entries win), then project, then user. At the same scope
    /// TOML wins over Markdown so existing hooks keep their meaning.
    pub fn load_with_paths(
        user_root: &Path,
        project_root: &Path,
        explicit_paths: &[PathBuf],
        cancelled: impl Fn() -> bool,
    ) -> Result<Self, SkillError> {
        if explicit_paths.len() > MAX_SKILL_FILES {
            return Err(SkillError::Invalid("too many explicit skill paths".into()));
        }
        let mut result = Self::default();
        let mut visited = 0;
        for (root, scope) in [
            (user_root, SkillScope::User),
            (project_root, SkillScope::Project),
        ]
        .into_iter()
        .chain(
            explicit_paths
                .iter()
                .map(|p| (p.as_path(), SkillScope::Explicit)),
        ) {
            check_cancel(&cancelled)?;
            if !root.exists() {
                if scope == SkillScope::Explicit {
                    return Err(SkillError::Invalid(format!(
                        "skill path does not exist: {}",
                        root.display()
                    )));
                }
                continue;
            }
            let mut local = BTreeMap::new();
            let mut markdown = BTreeMap::new();
            if fs::symlink_metadata(root)?.file_type().is_symlink() {
                return Err(SkillError::PathEscape(root.into()));
            }
            // The caller-selected root is the authority anchor. Canonicalize
            // ancestor aliases (for example macOS /var) before child checks.
            let root = root.canonicalize()?;
            if root.is_dir() {
                load_root(&root, scope != SkillScope::User, &mut local, &cancelled)?;
                discover_markdown(
                    &root,
                    scope,
                    &mut markdown,
                    &mut result.collisions,
                    &mut visited,
                    0,
                    &[],
                    &cancelled,
                )?;
            } else if root.file_name().is_some_and(|name| name == SKILL_MARKDOWN) {
                let metadata = read_metadata(&root, scope, &cancelled)?;
                markdown.insert(metadata.name.clone(), metadata);
            } else {
                return Err(SkillError::Invalid(
                    "explicit skill file must be SKILL.md".into(),
                ));
            }
            for (name, metadata) in markdown {
                if let Some(toml) = local.get(&name) {
                    result.collisions.push(SkillCollision {
                        name,
                        winner: toml.source.clone(),
                        loser: metadata.source,
                    });
                    continue;
                }
                result.remove_previous(&name, &metadata.source);
                result.markdown.insert(name, metadata);
            }
            for (name, skill) in local {
                result.remove_previous(&name, &skill.source);
                result.skills.insert(name, skill);
            }
            let index_bytes: usize = result
                .markdown
                .values()
                .map(|m| m.name.len() + m.description.len() + m.source.as_os_str().len() + 128)
                .sum();
            if result.markdown.len() + result.skills.len() > MAX_SKILL_FILES
                || index_bytes > MAX_SKILL_INDEX_BYTES
            {
                return Err(SkillError::Invalid("skill catalogue is too large".into()));
            }
        }
        check_cancel(&cancelled)?;
        Ok(result)
    }

    fn remove_previous(&mut self, name: &str, winner: &Path) {
        let loser = self
            .skills
            .remove(name)
            .map(|s| s.source)
            .or_else(|| self.markdown.remove(name).map(|s| s.source));
        if let Some(loser) = loser.filter(|p| p != winner) {
            self.collisions.push(SkillCollision {
                name: name.into(),
                winner: winner.into(),
                loser,
            });
        }
    }

    pub fn metadata(&self) -> impl Iterator<Item = &SkillMetadata> {
        self.markdown.values()
    }

    pub fn collisions(&self) -> &[SkillCollision] {
        &self.collisions
    }

    pub fn metadata_index(&self) -> String {
        let rows = self.markdown.values().filter(|s| !s.disable_model_invocation)
            .map(|s| format!("  <skill><name>{}</name><description>{}</description><location>{}</location></skill>",
                escape_xml(&s.name), escape_xml(&s.description), escape_xml(&s.source.to_string_lossy())))
            .collect::<Vec<_>>();
        if rows.is_empty() {
            String::new()
        } else {
            format!(
                "<available_skills>\n{}\n</available_skills>",
                rows.join("\n")
            )
        }
    }

    /// Read current bytes only after invocation is admitted. A stale index is
    /// refused: the caller must reload instead of silently using stale policy.
    pub fn load_body(
        &self,
        name: &str,
        invocation: SkillInvocation,
        arguments: &str,
        cancelled: impl Fn() -> bool,
    ) -> Result<SkillBody, SkillError> {
        check_cancel(&cancelled)?;
        let metadata = self
            .markdown
            .get(name)
            .ok_or_else(|| SkillError::Invalid(format!("unknown Markdown skill `{name}`")))?;
        if invocation == SkillInvocation::Model && metadata.disable_model_invocation {
            return Err(SkillError::Invalid(
                "skill disables model invocation".into(),
            ));
        }
        if arguments.len() > MAX_SKILL_INSTRUCTIONS_BYTES || arguments.contains('\0') {
            return Err(SkillError::Invalid(
                "skill arguments are invalid or too large".into(),
            ));
        }
        let text = read_skill_text(&metadata.source, &cancelled)?;
        if hash_text(&text) != metadata.source_hash {
            return Err(SkillError::Invalid(
                "skill changed since discovery; reload required".into(),
            ));
        }
        let (_, body) = parse_markdown(&text)?;
        check_cancel(&cancelled)?;
        Ok(SkillBody {
            metadata: metadata.clone(),
            body: body.into(),
            arguments: arguments.into(),
        })
    }

    pub fn manifests(&self) -> impl Iterator<Item = &SkillManifest> {
        self.skills.values().map(|skill| &skill.manifest)
    }

    pub fn instructions(&self) -> String {
        let mut sections = self
            .skills
            .values()
            .map(|skill| {
                format!(
                    "## Skill {} {}\n{}",
                    skill.manifest.name, skill.manifest.version, skill.manifest.instructions
                )
            })
            .collect::<Vec<_>>();
        let index = self.metadata_index();
        if !index.is_empty() {
            sections.push(index);
        }
        sections.join("\n\n")
    }

    /// Effective provider instructions in deterministic lifecycle order:
    /// skill bodies, prompt preparation, then compaction context notes.
    pub fn effective_instructions(&self) -> String {
        let mut sections = Vec::new();
        let instructions = self.instructions();
        if !instructions.is_empty() {
            sections.push(instructions);
        }
        let prompt = self.hook_outputs(|hooks| hooks.prompt_prefix.as_deref());
        if !prompt.is_empty() {
            sections.push(format!("## Prompt Preparation\n{}", prompt.join("\n")));
        }
        let context = self.hook_outputs(|hooks| hooks.context_prefix.as_deref());
        if !context.is_empty() {
            sections.push(format!("## Context Compaction\n{}", context.join("\n")));
        }
        sections.join("\n\n")
    }

    pub fn tool_allowed(&self, tool: &str) -> bool {
        let allowlists = self
            .skills
            .values()
            .filter(|skill| !skill.manifest.hooks.tool_allowlist.is_empty())
            .map(|skill| &skill.manifest.hooks.tool_allowlist)
            .collect::<Vec<_>>();
        allowlists.is_empty()
            || allowlists
                .iter()
                .all(|allowlist| allowlist.iter().any(|allowed| allowed == tool))
    }

    pub fn session_close_outputs(&self) -> Vec<String> {
        self.hook_outputs(|hooks| hooks.session_close.as_deref())
            .into_iter()
            .map(str::to_owned)
            .collect()
    }

    fn hook_outputs<'a>(
        &'a self,
        select: impl Fn(&'a SkillHooks) -> Option<&'a str>,
    ) -> Vec<&'a str> {
        self.skills
            .values()
            .filter_map(|skill| select(&skill.manifest.hooks))
            .collect()
    }

    pub fn prepare_prompt(&self, prompt: &str) -> String {
        let prefixes = self
            .skills
            .values()
            .filter_map(|skill| skill.manifest.hooks.prompt_prefix.as_deref())
            .collect::<Vec<_>>();
        if prefixes.is_empty() {
            prompt.to_owned()
        } else {
            format!("{}\n\n{prompt}", prefixes.join("\n"))
        }
    }
}

fn load_root(
    root: &Path,
    allow_override: bool,
    skills: &mut BTreeMap<String, LoadedSkill>,
    cancelled: &impl Fn() -> bool,
) -> Result<(), SkillError> {
    if !root.exists() {
        return Ok(());
    }
    let canonical_root = root.canonicalize()?;
    let mut entries = Vec::new();
    for entry in fs::read_dir(root)? {
        check_cancel(cancelled)?;
        if entries.len() >= MAX_SKILL_FILES {
            return Err(SkillError::Invalid(
                "skill root has too many entries".into(),
            ));
        }
        entries.push(entry?);
    }
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        check_cancel(cancelled)?;
        if entry.file_type()?.is_symlink() || !entry.file_type()?.is_dir() {
            continue;
        }
        let directory = entry.path().canonicalize()?;
        if !directory.starts_with(&canonical_root) {
            return Err(SkillError::PathEscape(entry.path()));
        }
        let manifest_path = directory.join(SKILL_MANIFEST);
        if !manifest_path.exists() {
            continue;
        }
        let metadata = fs::symlink_metadata(&manifest_path)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(SkillError::PathEscape(manifest_path));
        }
        let text = read_skill_text(&manifest_path, cancelled)?;
        let manifest: SkillManifest = toml::from_str(&text)?;
        manifest.validate()?;
        if skills.contains_key(&manifest.name) && !allow_override {
            return Err(SkillError::Duplicate(manifest.name));
        }
        skills.insert(
            manifest.name.clone(),
            LoadedSkill {
                manifest,
                source: manifest_path,
            },
        );
    }
    Ok(())
}

impl SkillBody {
    /// Resolve an existing relative resource under the selected skill directory.
    /// References remain inert: callers decide which read/tool operation to use.
    pub fn resolve_resource(&self, reference: &str) -> Result<PathBuf, SkillError> {
        use std::path::Component;
        let relative = Path::new(reference);
        if reference.is_empty()
            || reference.len() > 4096
            || reference.chars().any(char::is_control)
            || relative
                .components()
                .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
        {
            return Err(SkillError::PathEscape(relative.into()));
        }
        let path = checked_path(&self.metadata.base_dir.join(relative))?;
        if !path.starts_with(&self.metadata.base_dir) {
            return Err(SkillError::PathEscape(path));
        }
        Ok(path)
    }
}

fn check_cancel(cancelled: &impl Fn() -> bool) -> Result<(), SkillError> {
    if cancelled() {
        Err(SkillError::Cancelled)
    } else {
        Ok(())
    }
}

/// Resolve once and reject symbolic links at every supplied component.
fn checked_path(path: &Path) -> Result<PathBuf, SkillError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut cursor = PathBuf::new();
    for component in absolute.components() {
        cursor.push(component);
        if fs::symlink_metadata(&cursor)?.file_type().is_symlink() {
            return Err(SkillError::PathEscape(cursor));
        }
    }
    Ok(absolute.canonicalize()?)
}

pub(crate) fn read_skill_text(
    path: &Path,
    cancelled: &impl Fn() -> bool,
) -> Result<String, SkillError> {
    check_cancel(cancelled)?;
    let path = checked_path(path)?;
    let metadata = fs::metadata(&path)?;
    if !metadata.is_file() {
        return Err(SkillError::PathEscape(path));
    }
    if metadata.len() > MAX_SKILL_FILE_BYTES as u64 {
        return Err(SkillError::Invalid("skill file is too large".into()));
    }
    let mut file = open_skill_file(&path)?;
    if !file.metadata()?.is_file() {
        return Err(SkillError::Invalid("skill is not a regular file".into()));
    }
    let mut bytes = Vec::new();
    let mut chunk = [0; 8192];
    loop {
        check_cancel(cancelled)?;
        let read = file.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        if bytes.len() + read > MAX_SKILL_FILE_BYTES {
            return Err(SkillError::Invalid("skill file is too large".into()));
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
    let text = String::from_utf8(bytes)
        .map_err(|_| SkillError::Invalid("skill file is not UTF-8".into()))?;
    if text.contains('\0') {
        return Err(SkillError::Invalid("skill file contains NUL".into()));
    }
    check_cancel(cancelled)?;
    Ok(text)
}

/// Walk directory descriptors so a concurrent ancestor symlink replacement
/// cannot redirect a read outside the canonical source recorded at discovery.
#[cfg(unix)]
fn open_skill_file(path: &Path) -> Result<fs::File, SkillError> {
    use std::{
        ffi::CString,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::ffi::OsStrExt,
        },
        path::Component,
    };
    let parts = path
        .components()
        .filter_map(|part| match part {
            Component::Normal(value) => Some(value),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut directory = fs::File::open("/")?;
    for (index, part) in parts.iter().enumerate() {
        let name =
            CString::new(part.as_bytes()).map_err(|_| SkillError::PathEscape(path.into()))?;
        let flags = libc::O_RDONLY
            | libc::O_CLOEXEC
            | libc::O_NOFOLLOW
            | if index + 1 == parts.len() {
                libc::O_NONBLOCK
            } else {
                libc::O_DIRECTORY
            };
        // SAFETY: name is NUL-terminated, the parent descriptor stays alive
        // throughout openat, and a successful descriptor gets exactly one owner.
        let fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        // SAFETY: openat returned a fresh owned descriptor.
        directory = unsafe { fs::File::from_raw_fd(fd) };
    }
    Ok(directory)
}

#[cfg(not(unix))]
fn open_skill_file(path: &Path) -> Result<fs::File, SkillError> {
    Ok(fs::File::open(checked_path(path)?)?)
}

#[derive(Deserialize)]
struct MarkdownFrontmatter {
    #[serde(default, deserialize_with = "optional_yaml_string")]
    name: Option<String>,
    #[serde(deserialize_with = "yaml_string")]
    description: String,
    #[serde(default, rename = "disable-model-invocation")]
    disable_model_invocation: bool,
}

fn yaml_string<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    match yaml_serde::Value::deserialize(deserializer)? {
        yaml_serde::Value::String(value) => Ok(value),
        _ => Err(serde::de::Error::custom("expected a YAML string")),
    }
}

fn optional_yaml_string<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    yaml_string(deserializer).map(Some)
}

fn parse_markdown(text: &str) -> Result<(MarkdownFrontmatter, &str), SkillError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let first_end = text
        .find('\n')
        .ok_or_else(|| SkillError::Invalid("missing frontmatter".into()))?;
    if text[..first_end].trim_end_matches('\r') != "---" {
        return Err(SkillError::Invalid("missing YAML frontmatter".into()));
    }
    let mut offset = first_end + 1;
    for line in text[offset..].split_inclusive('\n') {
        if offset > 16 * 1024 {
            return Err(SkillError::Invalid("skill frontmatter is too large".into()));
        }
        if matches!(line.trim_end_matches(['\n', '\r']), "---" | "...") {
            let frontmatter: MarkdownFrontmatter =
                yaml_serde::from_str(&text[first_end + 1..offset]).map_err(|error| {
                    SkillError::Invalid(format!("invalid YAML frontmatter: {error}"))
                })?;
            let body = &text[offset + line.len()..];
            if frontmatter.description.trim().is_empty()
                || frontmatter.description.chars().count() > 1024
                || frontmatter
                    .description
                    .chars()
                    .any(|c| c.is_control() && c != '\n' && c != '\t')
            {
                return Err(SkillError::Invalid(
                    "skill description is empty, too long or contains controls".into(),
                ));
            }
            if body.trim().is_empty() || body.len() > MAX_SKILL_INSTRUCTIONS_BYTES {
                return Err(SkillError::Invalid(
                    "skill body is empty or too large".into(),
                ));
            }
            return Ok((frontmatter, body));
        }
        offset += line.len();
    }
    Err(SkillError::Invalid("unterminated YAML frontmatter".into()))
}

fn hash_text(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

fn read_metadata(
    path: &Path,
    scope: SkillScope,
    cancelled: &impl Fn() -> bool,
) -> Result<SkillMetadata, SkillError> {
    let source = checked_path(path)?;
    let text = read_skill_text(&source, cancelled)?;
    let (frontmatter, _) = parse_markdown(&text)?;
    let base_dir = source
        .parent()
        .ok_or_else(|| SkillError::PathEscape(source.clone()))?
        .to_path_buf();
    let name = frontmatter.name.unwrap_or_else(|| {
        base_dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    });
    if name.is_empty()
        || name.len() > 64
        || name.starts_with('-')
        || name.ends_with('-')
        || name.contains("--")
        || !name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
    {
        return Err(SkillError::Invalid(format!(
            "invalid Markdown skill name `{name}`"
        )));
    }
    Ok(SkillMetadata {
        name,
        description: frontmatter.description,
        disable_model_invocation: frontmatter.disable_model_invocation,
        source,
        base_dir,
        scope,
        source_hash: hash_text(&text),
    })
}

#[allow(clippy::too_many_arguments)]
fn discover_markdown(
    directory: &Path,
    scope: SkillScope,
    skills: &mut BTreeMap<String, SkillMetadata>,
    collisions: &mut Vec<SkillCollision>,
    visited: &mut usize,
    depth: usize,
    inherited: &[ignore::gitignore::Gitignore],
    cancelled: &impl Fn() -> bool,
) -> Result<(), SkillError> {
    check_cancel(cancelled)?;
    if depth > MAX_SKILL_DEPTH {
        return Err(SkillError::Invalid("skill discovery is too deep".into()));
    }
    let mut matchers = inherited.to_vec();
    let mut builder = ignore::gitignore::GitignoreBuilder::new(directory);
    for filename in [".gitignore", ".ignore", ".fdignore"] {
        let path = directory.join(filename);
        if path.exists() {
            let text = read_skill_text(&path, cancelled)?;
            for line in text.lines() {
                builder
                    .add_line(Some(path.clone()), line)
                    .map_err(|e| SkillError::Invalid(format!("invalid skill ignore rule: {e}")))?;
            }
        }
    }
    matchers.push(
        builder
            .build()
            .map_err(|e| SkillError::Invalid(format!("invalid skill ignore rules: {e}")))?,
    );
    let ignored = |path: &Path, is_dir: bool| {
        let mut result = false;
        for matcher in &matchers {
            let matched = matcher.matched(path, is_dir);
            if !matched.is_none() {
                result = matched.is_ignore();
            }
        }
        result
    };
    let declared = directory.join(SKILL_MARKDOWN);
    if declared.try_exists()? && !ignored(&declared, false) {
        let metadata = read_metadata(&declared, scope, cancelled)?;
        if let Some(winner) = skills.get(&metadata.name) {
            if winner.source != metadata.source {
                collisions.push(SkillCollision {
                    name: metadata.name,
                    winner: winner.source.clone(),
                    loser: metadata.source,
                });
            }
        } else {
            skills.insert(metadata.name.clone(), metadata);
        }
        return Ok(());
    }
    let mut entries = Vec::new();
    for entry in fs::read_dir(directory)? {
        check_cancel(cancelled)?;
        *visited += 1;
        if *visited > MAX_SKILL_FILES {
            return Err(SkillError::Invalid(
                "skill discovery has too many entries".into(),
            ));
        }
        entries.push(entry?);
    }
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        check_cancel(cancelled)?;
        let name = entry.file_name();
        let kind = entry.file_type()?;
        if name.to_string_lossy().starts_with('.')
            || name == "node_modules"
            || kind.is_symlink()
            || !kind.is_dir()
            || ignored(&entry.path(), true)
        {
            continue;
        }
        let child = checked_path(&entry.path())?;
        if !child.starts_with(directory) {
            return Err(SkillError::PathEscape(child));
        }
        discover_markdown(
            &child,
            scope,
            skills,
            collisions,
            visited,
            depth + 1,
            &matchers,
            cancelled,
        )?;
    }
    Ok(())
}

fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn validate_id(value: &str) -> Result<(), SkillError> {
    if value.is_empty()
        || value.len() > 128
        || value
            .chars()
            .any(|character| !(character.is_ascii_alphanumeric() || matches!(character, '_' | '-')))
    {
        return Err(SkillError::Invalid(format!("invalid identifier `{value}`")));
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum SkillError {
    #[error("skill loading cancelled")]
    Cancelled,
    #[error("skill I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("skill TOML: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("invalid skill: {0}")]
    Invalid(String),
    #[error("duplicate skill `{0}`")]
    Duplicate(String),
    #[error("skill path escapes its root: {0}")]
    PathEscape(PathBuf),
}

/// A selected relative resource read through the same descriptor-safe reader
/// as SKILL.md. Paths are never passed to a shell or interpreted as commands.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SkillResource {
    pub skill: String,
    pub source: PathBuf,
    pub source_hash: String,
    pub content: String,
}

impl SkillBody {
    pub fn read_resource(
        &self,
        reference: &str,
        cancelled: impl Fn() -> bool,
    ) -> Result<SkillResource, SkillError> {
        check_cancel(&cancelled)?;
        if Path::new(reference)
            .file_name()
            .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case(SKILL_MARKDOWN))
        {
            return Err(SkillError::Invalid(
                "use load_skill to read a skill body".into(),
            ));
        }
        let path = self.resolve_resource(reference)?;
        let content = read_skill_text(&path, &cancelled)?;
        if content.len() > MAX_SKILL_INSTRUCTIONS_BYTES {
            return Err(SkillError::Invalid("skill resource exceeds 64 KiB".into()));
        }
        check_cancel(&cancelled)?;
        Ok(SkillResource {
            skill: self.metadata.name.clone(),
            source: path,
            source_hash: hash_text(&content),
            content,
        })
    }
}

#[derive(Default)]
struct ActiveModelSkills {
    epoch: u64,
    skills: Option<SkillSet>,
    stale: std::collections::BTreeSet<String>,
    loaded: BTreeMap<String, SkillMetadata>,
}

/// Shared by the two registered tools and their owning Agent. Only a scoped
/// active turn grants access. Bodies are loaded on request and never cached.
#[derive(Clone, Default)]
pub struct ModelSkillTools(std::sync::Arc<std::sync::Mutex<ActiveModelSkills>>);

impl std::fmt::Debug for ModelSkillTools {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ModelSkillTools")
            .finish_non_exhaustive()
    }
}

pub struct ModelSkillTurn {
    shared: std::sync::Arc<std::sync::Mutex<ActiveModelSkills>>,
    epoch: u64,
}
impl Drop for ModelSkillTurn {
    fn drop(&mut self) {
        let mut state = self
            .shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.epoch == self.epoch {
            state.skills = None;
            state.stale.clear();
            state.loaded.clear();
        }
    }
}

impl ModelSkillTools {
    /// Registration checks both names before modifying the caller's registry.
    /// The Agent publishes its replacement ToolRuntime only after this succeeds.
    pub fn register(
        &self,
        registry: &mut crate::tools::ToolRegistry,
    ) -> Result<(), crate::tools::ToolError> {
        use crate::tools::Tool;
        let load = ModelSkillTool {
            shared: self.clone(),
            resource: false,
        };
        let read = ModelSkillTool {
            shared: self.clone(),
            resource: true,
        };
        for definition in [load.definition(), read.definition()] {
            definition.validate()?;
            if registry.definition(&definition.name).is_some() {
                return Err(crate::tools::ToolError::InvalidDefinition(format!(
                    "duplicate tool name `{}`",
                    definition.name
                )));
            }
        }
        registry.register(load)?;
        registry.register(read)?;
        Ok(())
    }

    pub fn begin_turn(
        &self,
        skills: SkillSet,
        stale: std::collections::BTreeSet<String>,
    ) -> Result<ModelSkillTurn, crate::tools::ToolError> {
        let mut state = self.lock()?;
        if state.skills.is_some() {
            return Err(crate::tools::ToolError::InvalidCall(
                "model skill turn is already active".into(),
            ));
        }
        let epoch = state.epoch.checked_add(1).ok_or_else(|| {
            crate::tools::ToolError::LimitExceeded("model skill turn epoch exhausted".into())
        })?;
        *state = ActiveModelSkills {
            epoch,
            skills: Some(skills),
            stale,
            loaded: BTreeMap::new(),
        };
        Ok(ModelSkillTurn {
            shared: self.0.clone(),
            epoch,
        })
    }

    pub fn loaded_metadata(
        &self,
        name: &str,
    ) -> Result<Option<SkillMetadata>, crate::tools::ToolError> {
        Ok(self.lock()?.loaded.get(name).cloned())
    }

    fn lock(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, ActiveModelSkills>, crate::tools::ToolError> {
        self.0.lock().map_err(|_| {
            crate::tools::ToolError::InvalidCall("model skill state lock is poisoned".into())
        })
    }
}

struct ModelSkillTool {
    shared: ModelSkillTools,
    resource: bool,
}

impl ModelSkillTool {
    fn run(
        &self,
        context: &crate::tools::ToolContext,
        args: &serde_json::Map<String, serde_json::Value>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<serde_json::Value, crate::tools::ToolError> {
        use crate::tools::{ToolError, ToolSideEffect};
        if cancelled() {
            return Err(ToolError::Cancelled);
        }
        let name = if self.resource {
            "read_skill_resource"
        } else {
            "load_skill"
        };
        // Existing Blueprint grants have no cross-root skill capability. This
        // rejects unknown/untrusted effects and unbound worker origins rather
        // than borrowing a workspace attachment grant for a different root.
        context.check_call_gate(name, ToolSideEffect::ReadOnly, args)?;
        let allowed = if self.resource {
            ["skill", "path"]
        } else {
            ["name", "arguments"]
        };
        if args.keys().any(|key| !allowed.contains(&key.as_str())) {
            return Err(ToolError::InvalidArguments(
                "unknown skill tool argument".into(),
            ));
        }
        let required = |key: &str, max: usize| -> Result<&str, ToolError> {
            args.get(key)
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.is_empty() && value.len() <= max && !value.contains('\0'))
                .ok_or_else(|| ToolError::InvalidArguments(format!("invalid skill tool `{key}`")))
        };
        let skill_name = required(if self.resource { "skill" } else { "name" }, 256)?;
        let (skills, loaded, epoch) = {
            let state = self.shared.lock()?;
            let skills = state.skills.as_ref().ok_or_else(|| {
                ToolError::InvalidCall("skill tools require an active provider turn".into())
            })?;
            if state.stale.contains(skill_name) {
                return Err(ToolError::InvalidArguments(
                    "skill changed since the saved selection; reload required".into(),
                ));
            }
            if !self.resource && !state.loaded.contains_key(skill_name) && state.loaded.len() >= 32
            {
                return Err(ToolError::LimitExceeded(
                    "at most 32 model-loaded skills per turn".into(),
                ));
            }
            (
                skills.clone(),
                state.loaded.get(skill_name).cloned(),
                state.epoch,
            )
        };
        let arguments = if self.resource {
            ""
        } else {
            match args.get("arguments") {
                None => "",
                Some(serde_json::Value::String(value))
                    if value.len() <= 8192 && !value.contains('\0') =>
                {
                    value
                }
                _ => {
                    return Err(ToolError::InvalidArguments(
                        "skill arguments must be a literal string of at most 8192 bytes".into(),
                    ));
                }
            }
        };
        if self.resource && loaded.is_none() {
            return Err(ToolError::InvalidCall(
                "load this skill in the current turn before reading its resources".into(),
            ));
        }
        // Both tools revalidate the source hash and disable-model-invocation;
        // a relative read cannot smuggle a disabled/stale skill body through.
        let body = skills
            .load_body(skill_name, SkillInvocation::Model, arguments, cancelled)
            .map_err(skill_tool_error)?;
        if self.resource {
            let loaded = loaded.expect("validated loaded skill");
            if loaded != body.metadata {
                return Err(ToolError::InvalidCall(
                    "selected skill provenance changed".into(),
                ));
            }
            let resource = body
                .read_resource(required("path", 4096)?, cancelled)
                .map_err(skill_tool_error)?;
            if cancelled() {
                return Err(ToolError::Cancelled);
            }
            let state = self.shared.lock()?;
            if state.epoch != epoch || state.skills.is_none() {
                return Err(ToolError::Cancelled);
            }
            Ok(serde_json::json!({"skill":body.metadata,"resource":resource}))
        } else {
            if cancelled() {
                return Err(ToolError::Cancelled);
            }
            let mut state = self.shared.lock()?;
            if state.epoch != epoch || state.skills.is_none() {
                return Err(ToolError::Cancelled);
            }
            if !state.loaded.contains_key(skill_name) && state.loaded.len() >= 32 {
                return Err(ToolError::LimitExceeded(
                    "at most 32 model-loaded skills per turn".into(),
                ));
            }
            state
                .loaded
                .insert(skill_name.into(), body.metadata.clone());
            Ok(
                serde_json::json!({"skill":body.metadata,"body":body.body,"arguments":body.arguments,
                "resource_tool":"read_skill_resource"}),
            )
        }
    }
}

impl crate::tools::Tool for ModelSkillTool {
    // Sequential is the default: a load followed by a relative read in one
    // batch is a dependency, despite both having read-only filesystem effects.
    fn definition(&self) -> crate::tools::ToolDefinition {
        let (name, description, schema) = if self.resource {
            (
                "read_skill_resource",
                "Read a bounded UTF-8 resource relative to a skill loaded with load_skill in this turn. Paths stay inside that skill directory; SKILL.md must use load_skill.",
                serde_json::json!({"type":"object","properties":{"skill":{"type":"string"},"path":{"type":"string"}},"required":["skill","path"],"additionalProperties":false}),
            )
        } else {
            (
                "load_skill",
                "Load the body of one available skill by exact name. Optional arguments are literal text. Disabled skills cannot be model-invoked. Load again in each new turn before using read_skill_resource.",
                serde_json::json!({"type":"object","properties":{"name":{"type":"string"},"arguments":{"type":"string"}},"required":["name"],"additionalProperties":false}),
            )
        };
        crate::tools::ToolDefinition {
            name: name.into(),
            description: description.into(),
            input_schema: schema,
            side_effect: crate::tools::ToolSideEffect::ReadOnly,
        }
    }
    fn invoke(
        &self,
        context: &crate::tools::ToolContext,
        args: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<serde_json::Value, crate::tools::ToolError> {
        self.run(context, args, &|| false)
    }
    fn invoke_cancellable(
        &self,
        context: &crate::tools::ToolContext,
        args: &serde_json::Map<String, serde_json::Value>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<serde_json::Value, crate::tools::ToolError> {
        self.run(context, args, cancelled)
    }
}

fn skill_tool_error(error: SkillError) -> crate::tools::ToolError {
    match error {
        SkillError::Cancelled => crate::tools::ToolError::Cancelled,
        other => crate::tools::ToolError::InvalidArguments(other.to_string()),
    }
}
