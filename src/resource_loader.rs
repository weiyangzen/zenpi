//! Atomic, explicitly selected skill/template/text-resource snapshots.
//!
//! Mapping: pi resource-loader.ts at revision
//! 23282f60782f02b9e22b787e4b22af441454fa16, source SHA256
//! 9acb127ae635c05235df4ab19e149a3a721b6e766a6ddfc03a26aff84d0a1f4e.
//! Atomic publication is a zenpi enhancement: the source mutates fields in order.

use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    prompt_templates::{PromptTemplates, TemplateError},
    skills::{SkillError, SkillSet, read_skill_text},
};

/// Hosts may serialize these selected paths in their existing settings owner.
/// This module itself writes no config, journal or temporary directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourcePaths {
    pub user_skills: PathBuf,
    pub project_skills: PathBuf,
    pub skill_paths: Vec<PathBuf>,
    pub user_templates: PathBuf,
    pub project_templates: PathBuf,
    pub template_paths: Vec<PathBuf>,
    pub text_resources: Vec<TextResourcePath>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextResourcePath {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextResource {
    pub name: String,
    pub source: PathBuf,
    pub source_hash: String,
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    Skill,
    Template,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceCollision {
    pub kind: ResourceKind,
    pub name: String,
    pub winner: PathBuf,
    pub loser: PathBuf,
}

/// Capture one Arc at turn admission; retain it for the entire admitted turn.
/// Reload can then publish a later generation without changing that turn's data.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResourceSnapshot {
    pub generation: u64,
    pub skills: SkillSet,
    pub templates: PromptTemplates,
    pub text_resources: BTreeMap<String, TextResource>,
}

impl ResourceSnapshot {
    pub fn collisions(&self) -> Vec<ResourceCollision> {
        self.skills
            .collisions()
            .iter()
            .map(|collision| ResourceCollision {
                kind: ResourceKind::Skill,
                name: collision.name.clone(),
                winner: collision.winner.clone(),
                loser: collision.loser.clone(),
            })
            .chain(
                self.templates
                    .collisions()
                    .iter()
                    .map(|collision| ResourceCollision {
                        kind: ResourceKind::Template,
                        name: collision.name.clone(),
                        winner: collision.winner.clone(),
                        loser: collision.loser.clone(),
                    }),
            )
            .collect()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ResourceError {
    #[error("resource reload cancelled")]
    Cancelled,
    #[error("invalid resource configuration: {0}")]
    Invalid(String),
    #[error("resource I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Skill(SkillError),
    #[error(transparent)]
    Template(TemplateError),
    #[error("text resource {path}: {error}")]
    Text {
        path: PathBuf,
        #[source]
        error: SkillError,
    },
}

#[derive(Debug, Clone)]
pub struct ResourceLoader {
    paths: ResourcePaths,
    current: Arc<ResourceSnapshot>,
}

impl ResourceLoader {
    /// Generation zero is empty and not yet loaded. Call reload before use.
    pub fn new(paths: ResourcePaths) -> Result<Self, ResourceError> {
        Ok(Self {
            paths: normalize_paths(paths)?,
            current: Arc::new(ResourceSnapshot::default()),
        })
    }

    pub fn paths(&self) -> &ResourcePaths {
        &self.paths
    }

    pub fn snapshot(&self) -> Arc<ResourceSnapshot> {
        Arc::clone(&self.current)
    }

    pub fn reload(
        &mut self,
        cancelled: impl Fn() -> bool,
    ) -> Result<Arc<ResourceSnapshot>, ResourceError> {
        self.reload_with_paths(self.paths.clone(), cancelled)
    }

    /// Changes to path selection are committed together with resource contents.
    /// Any read/parse/duplicate/cancellation error leaves both prior values intact.
    pub fn reload_with_paths(
        &mut self,
        paths: ResourcePaths,
        cancelled: impl Fn() -> bool,
    ) -> Result<Arc<ResourceSnapshot>, ResourceError> {
        check_cancel(&cancelled)?;
        let paths = normalize_paths(paths)?;
        let generation = self
            .current
            .generation
            .checked_add(1)
            .ok_or_else(|| ResourceError::Invalid("resource generation exhausted".into()))?;
        let skills = SkillSet::load_with_paths(
            &paths.user_skills,
            &paths.project_skills,
            &paths.skill_paths,
            &cancelled,
        )
        .map_err(|error| match error {
            SkillError::Cancelled => ResourceError::Cancelled,
            error => ResourceError::Skill(error),
        })?;
        check_cancel(&cancelled)?;
        let templates = PromptTemplates::load(
            &paths.user_templates,
            &paths.project_templates,
            &paths.template_paths,
            &cancelled,
        )
        .map_err(|error| match error {
            TemplateError::Cancelled => ResourceError::Cancelled,
            error => ResourceError::Template(error),
        })?;
        let mut text_resources = BTreeMap::new();
        let mut total = 0usize;
        for resource in &paths.text_resources {
            check_cancel(&cancelled)?;
            if text_resources.contains_key(&resource.name) {
                return Err(ResourceError::Invalid(format!(
                    "duplicate text resource `{}`",
                    resource.name
                )));
            }
            let content =
                read_skill_text(&resource.path, &cancelled).map_err(|error| match error {
                    SkillError::Cancelled => ResourceError::Cancelled,
                    error => ResourceError::Text {
                        path: resource.path.clone(),
                        error,
                    },
                })?;
            total += content.len();
            if total > 1024 * 1024 {
                return Err(ResourceError::Invalid(
                    "text resource snapshot exceeds 1 MiB".into(),
                ));
            }
            text_resources.insert(
                resource.name.clone(),
                TextResource {
                    name: resource.name.clone(),
                    source: resource.path.clone(),
                    source_hash: format!("{:x}", Sha256::digest(content.as_bytes())),
                    content,
                },
            );
        }
        check_cancel(&cancelled)?;
        let next = Arc::new(ResourceSnapshot {
            generation,
            skills,
            templates,
            text_resources,
        });
        self.paths = paths;
        self.current = Arc::clone(&next);
        Ok(next)
    }
}

fn check_cancel(cancelled: &impl Fn() -> bool) -> Result<(), ResourceError> {
    if cancelled() {
        Err(ResourceError::Cancelled)
    } else {
        Ok(())
    }
}

fn normalize_paths(mut paths: ResourcePaths) -> Result<ResourcePaths, ResourceError> {
    if paths.skill_paths.len() > crate::skills::MAX_SKILL_FILES
        || paths.template_paths.len() > crate::prompt_templates::MAX_TEMPLATE_COUNT
        || paths.text_resources.len() > 64
    {
        return Err(ResourceError::Invalid("too many resource paths".into()));
    }
    for path in [
        &mut paths.user_skills,
        &mut paths.project_skills,
        &mut paths.user_templates,
        &mut paths.project_templates,
    ]
    .into_iter()
    .chain(paths.skill_paths.iter_mut())
    .chain(paths.template_paths.iter_mut())
    {
        if path.as_os_str().is_empty() {
            return Err(ResourceError::Invalid("empty resource path".into()));
        }
        *path = std::path::absolute(&path)?;
    }
    for resource in &mut paths.text_resources {
        if resource.name.is_empty()
            || resource.name.len() > 128
            || !resource
                .name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
            || resource.path.as_os_str().is_empty()
        {
            return Err(ResourceError::Invalid(
                "invalid text resource name or path".into(),
            ));
        }
        // Canonicalize only the parent alias, retaining the final filename for
        // the no-symlink read check. Missing paths are reported during reload.
        let absolute = std::path::absolute(&resource.path)?;
        resource.path = match (absolute.parent(), absolute.file_name()) {
            (Some(parent), Some(name)) if parent.exists() => parent.canonicalize()?.join(name),
            _ => absolute,
        };
    }
    Ok(paths)
}
