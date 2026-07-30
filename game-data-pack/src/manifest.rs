use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Context;
use serde::{Deserialize, Serialize};

/// Written above the serialized TOML by [`Manifest::save`]. `toml` cannot round
/// trip comments, so a `--latest` run would otherwise silently strip the
/// regeneration instructions from `data/manifest.toml`.
pub const HEADER: &str = "\
# Pinned upstream sources for the packs in this directory.
# Regenerate packs with: cargo run --release -p game-data-pack -- --pinned
# Update pins + regenerate with: cargo run --release -p game-data-pack -- --latest
#
# `sparse_paths` is optional: when it is absent the generator derives the
# per-language sheet patterns from SHEETS (see game-data-pack/src/fetch.rs).
# `checkout_subdir` places a source inside the assembled ffxiv-datamining tree.
#
# This header is rewritten by `Manifest::save`; comments added below it are NOT
# preserved across a --latest run.
";

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
    // Skipped when empty so a `--latest` rewrite stays a one-line-per-source diff.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sparse_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkout_subdir: Option<String>,
}

impl Manifest {
    /// Load and parse a manifest from a TOML file at `path`.
    pub fn load(path: &Path) -> anyhow::Result<Manifest> {
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("reading manifest at {}", path.display()))?;
        toml::from_str(&contents).with_context(|| format!("parsing manifest at {}", path.display()))
    }

    /// Serialize and write this manifest as TOML to `path`, prefixed with
    /// [`HEADER`].
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let body = toml::to_string_pretty(self).context("serializing manifest")?;
        std::fs::write(path, format!("{HEADER}\n{body}"))
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

    #[test]
    fn save_keeps_the_regeneration_instructions() {
        let manifest = Manifest::load(&manifest_path()).expect("manifest should load");

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("manifest.toml");
        manifest.save(&path).expect("manifest should save");

        let written = std::fs::read_to_string(&path).expect("read back");
        assert!(
            written.starts_with(HEADER),
            "saved manifest lost its header:\n{written}"
        );
        assert!(written.contains("[sources.ffxiv-datamining]"));
    }

    #[test]
    fn checked_in_manifest_carries_the_same_header() {
        let contents = std::fs::read_to_string(manifest_path()).expect("read manifest");
        assert!(
            contents.starts_with(HEADER),
            "data/manifest.toml has drifted from Manifest::HEADER; a --latest run would rewrite it"
        );
    }
}
