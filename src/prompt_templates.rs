//! Independent Markdown prompt templates and bounded, one-pass substitution.
//!
//! Source behavior: pi packages/coding-agent/src/core/prompt-templates.ts,
//! SHA256 e94b8504b97fe668b04577891b7029abc7d11ac795e728982d2615a13ec1528a.
//! This module never authorizes slash controls or evaluates shell syntax.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::skills::{SkillError, SkillScope, read_skill_text};

pub const MAX_TEMPLATE_INPUT_BYTES: usize = 64 * 1024;
pub const MAX_TEMPLATE_OUTPUT_BYTES: usize = 1024 * 1024;
pub const MAX_TEMPLATE_COUNT: usize = 1024;
const MAX_CATALOGUE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptTemplate {
    pub name: String,
    pub description: String,
    pub argument_hint: Option<String>,
    pub content: String,
    pub source: PathBuf,
    pub source_hash: String,
    pub scope: SkillScope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateCollision {
    pub name: String,
    pub winner: PathBuf,
    pub loser: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateExpansion {
    pub name: String,
    pub content: String,
    pub source: PathBuf,
    pub source_hash: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PromptTemplates {
    templates: BTreeMap<String, PromptTemplate>,
    collisions: Vec<TemplateCollision>,
}

#[derive(Debug, thiserror::Error)]
pub enum TemplateError {
    #[error("prompt template loading cancelled")]
    Cancelled,
    #[error("invalid prompt template: {0}")]
    Invalid(String),
    #[error("prompt template I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("prompt template {path}: {error}")]
    Source {
        path: PathBuf,
        #[source]
        error: SkillError,
    },
    #[error("duplicate prompt template `{name}` at the same priority: {first} and {second}")]
    Duplicate {
        name: String,
        first: PathBuf,
        second: PathBuf,
    },
}

impl PromptTemplates {
    /// Load direct .md children only. Explicit > project > user. Duplicate
    /// names at one priority fail; repeated references to the same file dedupe.
    pub fn load(
        user_root: &Path,
        project_root: &Path,
        explicit: &[PathBuf],
        cancelled: impl Fn() -> bool,
    ) -> Result<Self, TemplateError> {
        if explicit.len() > MAX_TEMPLATE_COUNT {
            return Err(TemplateError::Invalid("too many explicit paths".into()));
        }
        let mut result = Self::default();
        let mut visited = 0usize;
        for (root, scope) in [
            (user_root, SkillScope::User),
            (project_root, SkillScope::Project),
        ]
        .into_iter()
        .chain(explicit.iter().map(|p| (p.as_path(), SkillScope::Explicit)))
        {
            check_cancel(&cancelled)?;
            if !root.try_exists()? {
                if scope == SkillScope::Explicit {
                    return Err(TemplateError::Invalid(format!(
                        "explicit path does not exist: {}",
                        root.display()
                    )));
                }
                continue;
            }
            if fs::symlink_metadata(root)?.file_type().is_symlink() {
                return Err(TemplateError::Invalid(format!(
                    "symlink template path: {}",
                    root.display()
                )));
            }
            let root = root.canonicalize()?;
            let paths = if root.is_dir() {
                let mut entries = Vec::new();
                for entry in fs::read_dir(&root)? {
                    check_cancel(&cancelled)?;
                    visited += 1;
                    if visited > MAX_TEMPLATE_COUNT * 4 {
                        return Err(TemplateError::Invalid("too many directory entries".into()));
                    }
                    let entry = entry?;
                    if entry.path().extension().is_some_and(|ext| ext == "md") {
                        if entry.file_type()?.is_symlink() {
                            return Err(TemplateError::Invalid(format!(
                                "symlink template path: {}",
                                entry.path().display()
                            )));
                        }
                        if entry.file_type()?.is_file() {
                            entries.push(entry.path());
                        }
                    }
                }
                entries.sort();
                entries
            } else if root.is_file() && root.extension().is_some_and(|ext| ext == "md") {
                vec![root]
            } else {
                return Err(TemplateError::Invalid(
                    "template path must be a directory or .md file".into(),
                ));
            };
            for path in paths {
                check_cancel(&cancelled)?;
                let template = read_template(&path, scope, &cancelled)?;
                if let Some(previous) = result.templates.get(&template.name) {
                    if previous.source == template.source {
                        continue;
                    }
                    if previous.scope == template.scope {
                        return Err(TemplateError::Duplicate {
                            name: template.name,
                            first: previous.source.clone(),
                            second: template.source,
                        });
                    }
                    result.collisions.push(TemplateCollision {
                        name: template.name.clone(),
                        winner: template.source.clone(),
                        loser: previous.source.clone(),
                    });
                }
                result.templates.insert(template.name.clone(), template);
                let bytes: usize = result
                    .templates
                    .values()
                    .map(|t| {
                        t.content.len()
                            + t.description.len()
                            + t.source.as_os_str().len()
                            + t.name.len()
                            + t.argument_hint.as_ref().map_or(0, String::len)
                    })
                    .sum();
                if result.templates.len() > MAX_TEMPLATE_COUNT || bytes > MAX_CATALOGUE_BYTES {
                    return Err(TemplateError::Invalid(
                        "template catalogue is too large".into(),
                    ));
                }
            }
        }
        check_cancel(&cancelled)?;
        Ok(result)
    }

    pub fn templates(&self) -> impl Iterator<Item = &PromptTemplate> {
        self.templates.values()
    }

    pub fn get(&self, name: &str) -> Option<&PromptTemplate> {
        self.templates.get(name)
    }

    pub fn collisions(&self) -> &[TemplateCollision] {
        &self.collisions
    }

    /// Expand an exact template command. Hosts must give built-in controls and
    /// explicit skill commands priority before calling this method; None must
    /// preserve their existing unknown-slash refusal, not become a prompt.
    pub fn expand(
        &self,
        input: &str,
        cancelled: impl Fn() -> bool,
    ) -> Result<Option<TemplateExpansion>, TemplateError> {
        check_cancel(&cancelled)?;
        bounded_input(input)?;
        let Some(command) = input.strip_prefix('/') else {
            return Ok(None);
        };
        let end = command.find(char::is_whitespace).unwrap_or(command.len());
        let Some(template) = self.templates.get(&command[..end]) else {
            return Ok(None);
        };
        let args = parse_command_args(&command[end..])?;
        Ok(Some(TemplateExpansion {
            name: template.name.clone(),
            content: substitute_args(&template.content, &args, cancelled)?,
            source: template.source.clone(),
            source_hash: template.source_hash.clone(),
        }))
    }
}

fn check_cancel(cancelled: &impl Fn() -> bool) -> Result<(), TemplateError> {
    if cancelled() {
        Err(TemplateError::Cancelled)
    } else {
        Ok(())
    }
}

fn bounded_input(input: &str) -> Result<(), TemplateError> {
    if input.len() > MAX_TEMPLATE_INPUT_BYTES || input.contains('\0') {
        Err(TemplateError::Invalid(
            "input is too large or contains NUL".into(),
        ))
    } else {
        Ok(())
    }
}

/// Match the source template grammar: quotes group whitespace, empty quotes
/// disappear, an unmatched quote consumes to EOF, backslashes remain literal.
/// This is deliberately separate from zenpi's stricter control-command parser.
pub fn parse_command_args(input: &str) -> Result<Vec<String>, TemplateError> {
    bounded_input(input)?;
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    for character in input.chars() {
        if let Some(open) = quote {
            if character == open {
                quote = None;
            } else {
                current.push(character);
            }
        } else if matches!(character, '\'' | '"') {
            quote = Some(character);
        } else if character.is_whitespace() {
            if !current.is_empty() {
                args.push(std::mem::take(&mut current));
            }
        } else {
            current.push(character);
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    Ok(args)
}

/// Scan only the original template. Arguments and fallback strings are appended
/// as opaque data, so inserted $1/$@/shell characters never trigger another pass.
pub fn substitute_args(
    content: &str,
    args: &[String],
    cancelled: impl Fn() -> bool,
) -> Result<String, TemplateError> {
    bounded_input(content)?;
    let arg_bytes = args
        .iter()
        .try_fold(0usize, |size, arg| {
            size.checked_add(arg.len()).and_then(|n| n.checked_add(1))
        })
        .ok_or_else(|| TemplateError::Invalid("arguments are too large".into()))?;
    if arg_bytes > MAX_TEMPLATE_INPUT_BYTES || args.iter().any(|arg| arg.contains('\0')) {
        return Err(TemplateError::Invalid(
            "arguments are too large or contain NUL".into(),
        ));
    }
    let all = args.join(" ");
    let mut output = String::new();
    let mut cursor = 0;
    while cursor < content.len() {
        check_cancel(&cancelled)?;
        let tail = &content[cursor..];
        if let Some((length, replacement)) = placeholder(tail, args, &all) {
            append_bounded(&mut output, &replacement)?;
            cursor += length;
        } else {
            let length = tail
                .chars()
                .next()
                .expect("nonempty template suffix")
                .len_utf8();
            append_bounded(&mut output, &tail[..length])?;
            cursor += length;
        }
    }
    check_cancel(&cancelled)?;
    Ok(output)
}

fn append_bounded(output: &mut String, value: &str) -> Result<(), TemplateError> {
    if value.len() > MAX_TEMPLATE_OUTPUT_BYTES.saturating_sub(output.len()) {
        return Err(TemplateError::Invalid(
            "expanded template is too large".into(),
        ));
    }
    output.push_str(value);
    Ok(())
}

fn digits(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit())
}

fn positional<'a>(target: &str, args: &'a [String]) -> &'a str {
    target
        .parse::<usize>()
        .ok()
        .and_then(|index| index.checked_sub(1))
        .and_then(|index| args.get(index))
        .map_or("", String::as_str)
}

fn placeholder(input: &str, args: &[String], all: &str) -> Option<(usize, String)> {
    let tail = input.strip_prefix('$')?;
    if let Some(braced) = tail.strip_prefix('{') {
        let end = braced.find('}')?;
        let expression = &braced[..end];
        if let Some((target, fallback)) = expression.split_once(":-") {
            let value = match target {
                "@" | "ARGUMENTS" => all,
                t if digits(t) => positional(t, args),
                _ => return None,
            };
            return Some((
                end + 3,
                if value.is_empty() { fallback } else { value }.into(),
            ));
        }
        let slice = expression.strip_prefix("@:")?;
        let parts = slice.split(':').collect::<Vec<_>>();
        if !(1..=2).contains(&parts.len()) || !parts.iter().all(|part| digits(part)) {
            return None;
        }
        let start = parts[0]
            .parse::<usize>()
            .unwrap_or(usize::MAX)
            .saturating_sub(1)
            .min(args.len());
        let length = parts.get(1).map_or(args.len(), |length| {
            length.parse::<usize>().unwrap_or(usize::MAX)
        });
        let stop = start.saturating_add(length).min(args.len());
        return Some((end + 3, args[start..stop].join(" ")));
    }
    if tail.starts_with("ARGUMENTS") {
        return Some((10, all.into()));
    }
    if tail.starts_with('@') {
        return Some((2, all.into()));
    }
    let count = tail.bytes().take_while(u8::is_ascii_digit).count();
    if count > 0 {
        Some((count + 1, positional(&tail[..count], args).into()))
    } else {
        None
    }
}

fn read_template(
    path: &Path,
    scope: SkillScope,
    cancelled: &impl Fn() -> bool,
) -> Result<PromptTemplate, TemplateError> {
    let text = read_skill_text(path, cancelled).map_err(|error| match error {
        SkillError::Cancelled => TemplateError::Cancelled,
        error => TemplateError::Source {
            path: path.into(),
            error,
        },
    })?;
    let name = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if name.is_empty()
        || name.len() > 128
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return Err(TemplateError::Invalid(format!(
            "invalid template filename: {}",
            path.display()
        )));
    }
    // The source helper normalizes newlines, strips BOM, and trims body only
    // when frontmatter is present. Reject malformed frontmatter rather than
    // silently turning it into model instructions.
    let normalized = text
        .strip_prefix('\u{feff}')
        .unwrap_or(&text)
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let (metadata, body) = split_frontmatter(&normalized)
        .map_err(|error| TemplateError::Invalid(format!("{}: {error}", path.display())))?;
    bounded_input(body)?;
    if body.trim().is_empty() {
        return Err(TemplateError::Invalid(format!(
            "empty template: {}",
            path.display()
        )));
    }
    let description = string_field(&metadata, "description")?
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            let first = body
                .lines()
                .find(|line| !line.trim().is_empty())
                .unwrap_or_default();
            let mut short = first.chars().take(60).collect::<String>();
            if first.chars().count() > 60 {
                short.push_str("...");
            }
            short
        });
    let argument_hint = string_field(&metadata, "argument-hint")?.filter(|value| !value.is_empty());
    if description.len() > 4096 || argument_hint.as_ref().is_some_and(|hint| hint.len() > 1024) {
        return Err(TemplateError::Invalid(
            "template metadata is too large".into(),
        ));
    }
    check_cancel(cancelled)?;
    Ok(PromptTemplate {
        name: name.into(),
        description,
        argument_hint,
        content: body.into(),
        source: path.into(),
        source_hash: format!("{:x}", Sha256::digest(text.as_bytes())),
        scope,
    })
}

fn split_frontmatter(text: &str) -> Result<(yaml_serde::Mapping, &str), TemplateError> {
    if !text.starts_with("---\n") {
        return Ok((yaml_serde::Mapping::new(), text));
    }
    let mut offset = 4;
    for line in text[offset..].split_inclusive('\n') {
        if offset > 16 * 1024 {
            return Err(TemplateError::Invalid("frontmatter is too large".into()));
        }
        if matches!(line.trim_end_matches('\n'), "---" | "...") {
            let header = &text[4..offset];
            let metadata = if header.trim().is_empty() {
                yaml_serde::Mapping::new()
            } else {
                yaml_serde::from_str(header)
                    .map_err(|e| TemplateError::Invalid(format!("invalid YAML: {e}")))?
            };
            return Ok((metadata, text[offset + line.len()..].trim()));
        }
        offset += line.len();
    }
    Err(TemplateError::Invalid(
        "unterminated YAML frontmatter".into(),
    ))
}

fn string_field(
    metadata: &yaml_serde::Mapping,
    field: &str,
) -> Result<Option<String>, TemplateError> {
    match metadata.get(yaml_serde::Value::String(field.into())) {
        None => Ok(None),
        Some(yaml_serde::Value::String(value)) => Ok(Some(value.clone())),
        _ => Err(TemplateError::Invalid(format!(
            "{field} must be a YAML string"
        ))),
    }
}
