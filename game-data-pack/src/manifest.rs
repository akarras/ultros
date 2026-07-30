use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Context;
use serde::{Deserialize, Serialize};

/// Pin manifest for the upstream game-data sources packaged under `data/`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub sources: BTreeMap<String, Source>,
}

/// A single pinned upstream source (a git repo at a specific commit).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub url: String,
    pub sha: String,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub sparse_paths: Vec<String>,
    #[serde(default)]
    pub checkout_subdir: Option<String>,
}

impl Manifest {
    /// Load and parse a manifest from a TOML file at `path`.
    pub fn load(path: &Path) -> anyhow::Result<Manifest> {
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("reading manifest at {}", path.display()))?;
        toml::from_str(&contents).with_context(|| format!("parsing manifest at {}", path.display()))
    }

    /// Serialize and write this manifest as TOML to `path`.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let contents = toml::to_string_pretty(self).context("serializing manifest")?;
        std::fs::write(path, contents)
            .with_context(|| format!("writing manifest to {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest_path() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/manifest.toml")
    }

    #[test]
    fn load_reads_five_sources() {
        let manifest = Manifest::load(&manifest_path()).expect("manifest should load");
        assert_eq!(manifest.sources.len(), 5);
    }

    #[test]
    fn ko_source_has_refactor_branch() {
        let manifest = Manifest::load(&manifest_path()).expect("manifest should load");
        let ko = manifest
            .sources
            .get("ffxiv-datamining-ko")
            .expect("ko source present");
        assert_eq!(ko.branch, Some("refactor".to_string()));
        assert_eq!(ko.checkout_subdir, Some("csv/ko".to_string()));
    }

    #[test]
    fn round_trips_through_save_and_load() {
        let manifest = Manifest::load(&manifest_path()).expect("manifest should load");

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("manifest.toml");
        manifest.save(&path).expect("manifest should save");

        let reloaded = Manifest::load(&path).expect("manifest should reload");
        assert_eq!(manifest, reloaded);
    }
}
