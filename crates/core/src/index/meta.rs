use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{CorpusKind, IndexError, IndexKind};

const META_FILE: &str = "meta.json";
const STORE_VERSION: u32 = 1;

/// Persistent corpus configuration for an index store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreMeta {
    pub version: u32,
    pub root: PathBuf,
    pub corpus_kind: CorpusKind,
    pub follow_links: bool,
    #[serde(default)]
    pub indexes: Vec<IndexKind>,
}

impl StoreMeta {
    /// Create a new `StoreMeta` with the current store version.
    #[must_use]
    pub const fn new(
        root: PathBuf,
        corpus_kind: CorpusKind,
        follow_links: bool,
        indexes: Vec<IndexKind>,
    ) -> Self {
        Self {
            version: STORE_VERSION,
            root,
            corpus_kind,
            follow_links,
            indexes,
        }
    }

    /// Path to `meta.json` within `dir`.
    #[must_use]
    pub fn path(dir: &Path) -> PathBuf {
        dir.join(META_FILE)
    }

    /// Read from `dir/meta.json`.
    ///
    /// # Errors
    ///
    /// Returns an error if the file is missing or malformed.
    pub fn read(dir: &Path) -> crate::Result<Self> {
        let meta_path = Self::path(dir);
        let raw = std::fs::read_to_string(&meta_path)?;
        serde_json::from_str(&raw).map_err(|e| {
            crate::Error::Index(IndexError::InvalidManifest {
                path: meta_path,
                source: e,
            })
        })
    }

    /// Write to `dir/meta.json`.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization or writing fails.
    pub fn write(&self, dir: &Path) -> crate::Result<()> {
        let meta_path = Self::path(dir);
        let json = serde_json::to_vec_pretty(self).map_err(|e| {
            crate::Error::Index(IndexError::InvalidManifest {
                path: meta_path.clone(),
                source: e,
            })
        })?;
        std::fs::write(&meta_path, json)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn read_write_roundtrip() {
        let tmp = TempDir::new().expect("create temp dir");

        let meta = StoreMeta::new(
            PathBuf::from("/some/root"),
            CorpusKind::Directory,
            true,
            vec![IndexKind::Trigram],
        );
        meta.write(tmp.path()).expect("write meta");

        let loaded = StoreMeta::read(tmp.path()).expect("read meta");
        assert_eq!(loaded.version, meta.version);
        assert_eq!(loaded.root, meta.root);
        assert_eq!(loaded.corpus_kind, meta.corpus_kind);
        assert_eq!(loaded.follow_links, meta.follow_links);
        assert_eq!(loaded.indexes, meta.indexes);
    }

    #[test]
    fn path_returns_meta_json() {
        let p = StoreMeta::path(Path::new("/foo/bar"));
        assert_eq!(p, PathBuf::from("/foo/bar/meta.json"));
    }
}
