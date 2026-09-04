//! Bounded project/user skill manifests and lifecycle hooks.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

pub const SKILL_MANIFEST: &str = "skill.toml";
pub const MAX_SKILL_INSTRUCTIONS_BYTES: usize = 64 * 1024;

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
}

impl SkillSet {
    /// Load user skills first and project skills second so the project-local
    /// manifest deliberately wins for an identical skill ID.
    pub fn load(user_root: &Path, project_root: &Path) -> Result<Self, SkillError> {
        let mut skills = BTreeMap::new();
        load_root(user_root, false, &mut skills)?;
        load_root(project_root, true, &mut skills)?;
        Ok(Self { skills })
    }

    pub fn manifests(&self) -> impl Iterator<Item = &SkillManifest> {
        self.skills.values().map(|skill| &skill.manifest)
    }

    pub fn instructions(&self) -> String {
        self.skills
            .values()
            .map(|skill| {
                format!(
                    "## Skill {} {}\n{}",
                    skill.manifest.name, skill.manifest.version, skill.manifest.instructions
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n")
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
) -> Result<(), SkillError> {
    if !root.exists() {
        return Ok(());
    }
    let canonical_root = root.canonicalize()?;
    for entry in fs::read_dir(root)? {
        let entry = entry?;
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
        let text = fs::read_to_string(&manifest_path)?;
        if text.len() > MAX_SKILL_INSTRUCTIONS_BYTES * 2 {
            return Err(SkillError::Invalid("skill manifest is too large".into()));
        }
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
