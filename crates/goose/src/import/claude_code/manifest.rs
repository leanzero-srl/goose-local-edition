//! Import provenance manifest — `<config_dir>/.import/claude-code.json`.
//!
//! Records every artifact the importer wrote (type, name, target path, source content-hash). This makes
//! the importer safe to run repeatedly (the reconcile key for idempotency) and powers a precise revert:
//! the manifest enumerates exactly what the importer created, so nothing else is ever touched.

use super::ImportType;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    /// The [`ImportType`] label (e.g. "skills").
    pub import_type: String,
    /// The artifact name (skill name, memory category, server name).
    pub name: String,
    /// The target path written (for revert).
    pub target: String,
    /// The source content-hash at import time (the reconcile key).
    pub hash: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Manifest {
    #[serde(default)]
    pub entries: Vec<ManifestEntry>,
}

impl Manifest {
    /// The on-disk manifest path within a config dir.
    pub fn path_in(config_dir: &Path) -> PathBuf {
        config_dir.join(".import").join("claude-code.json")
    }

    /// Load the manifest for a config dir, or an empty one if absent/unreadable.
    pub fn load(config_dir: &Path) -> Self {
        let path = Self::path_in(config_dir);
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Persist the manifest under a config dir.
    pub fn save(&self, config_dir: &Path) -> Result<()> {
        let path = Self::path_in(config_dir);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    /// The recorded source-hash for a (type, name), if the importer wrote it before.
    pub fn hash_for(&self, ty: ImportType, name: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|e| e.import_type == ty.label() && e.name == name)
            .map(|e| e.hash.as_str())
    }

    /// Insert or replace the entry for a (type, name).
    pub fn upsert(&mut self, ty: ImportType, name: &str, target: &str, hash: &str) {
        self.entries
            .retain(|e| !(e.import_type == ty.label() && e.name == name));
        self.entries.push(ManifestEntry {
            import_type: ty.label().to_string(),
            name: name.to_string(),
            target: target.to_string(),
            hash: hash.to_string(),
        });
    }
}
