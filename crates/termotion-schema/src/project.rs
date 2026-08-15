use std::path::{Path, PathBuf};

use serde::Deserialize;

/// `termotion.yaml`. Every field is optional; this is a defaults layer.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    #[serde(default)]
    pub project: Option<ProjectMeta>,
    #[serde(default)]
    pub defaults: Option<ProjectDefaults>,
    #[serde(default)]
    pub paths: Option<ProjectPaths>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProjectMeta {
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProjectDefaults {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<u32>,
    pub theme: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProjectPaths {
    pub scenes: Option<String>,
    pub themes: Option<String>,
    pub assets: Option<String>,
    pub output: Option<String>,
}

impl ProjectConfig {
    /// Walks up from `start` looking for `termotion.yaml`. A malformed project
    /// file is ignored rather than blocking a render — it is a defaults layer.
    pub fn load_nearest(start: &Path) -> Option<(PathBuf, ProjectConfig)> {
        let start = start.canonicalize().unwrap_or_else(|_| start.to_path_buf());
        let mut dir = if start.is_dir() {
            Some(start.as_path())
        } else {
            start.parent()
        };

        while let Some(current) = dir {
            let candidate = current.join("termotion.yaml");
            if candidate.is_file() {
                let source = std::fs::read_to_string(&candidate).ok()?;
                let config: ProjectConfig = serde_yaml_ng::from_str(&source).ok()?;
                return Some((current.to_path_buf(), config));
            }
            dir = current.parent();
        }
        None
    }

    pub fn themes_dir(&self, root: &Path) -> PathBuf {
        let rel = self
            .paths
            .as_ref()
            .and_then(|p| p.themes.clone())
            .unwrap_or_else(|| "./themes".to_string());
        root.join(rel)
    }

    pub fn output_dir(&self, root: &Path) -> PathBuf {
        let rel = self
            .paths
            .as_ref()
            .and_then(|p| p.output.clone())
            .unwrap_or_else(|| "./dist".to_string());
        root.join(rel)
    }
}
