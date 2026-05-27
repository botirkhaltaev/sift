use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::meta::StoreMeta;
use super::snapshot::{self, SnapshotStore};
use super::{CorpusKind, IndexError};

const MANIFEST_FILE: &str = "manifest.json";

/// Manifest written into each snapshot directory listing the indexes it
/// contains.
#[derive(Debug, Serialize, Deserialize)]
struct SnapshotManifest {
    id: String,
    indexes: Vec<String>,
}

/// Index lifecycle orchestrator backed by a [`SnapshotStore`] for atomic
/// persistence and [`StoreMeta`] for corpus configuration.
pub struct IndexStore {
    snapshots: SnapshotStore,
    sift_dir: PathBuf,
}

impl IndexStore {
    /// Open an existing store at `sift_dir`.
    ///
    /// # Errors
    ///
    /// Returns an error if `CURRENT` exists but cannot be read.
    pub fn open(sift_dir: &Path) -> crate::Result<Self> {
        let snapshots = SnapshotStore::open(sift_dir)?;
        Ok(Self {
            snapshots,
            sift_dir: sift_dir.to_path_buf(),
        })
    }

    /// Open an existing store or create a new one at `sift_dir`.
    ///
    /// # Errors
    ///
    /// Returns an error if the store directory cannot be created or metadata
    /// cannot be written.
    pub fn open_or_create(
        sift_dir: &Path,
        root: &Path,
        corpus_kind: CorpusKind,
        follow_links: bool,
        indexes: &[super::IndexKind],
    ) -> crate::Result<Self> {
        std::fs::create_dir_all(sift_dir)?;

        if !StoreMeta::path(sift_dir).exists() {
            let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
            let meta = StoreMeta::new(canonical_root, corpus_kind, follow_links, indexes.to_vec());
            meta.write(sift_dir)?;
        }

        Self::open(sift_dir)
    }

    #[must_use]
    pub fn current_id(&self) -> Option<&str> {
        self.snapshots.current_id()
    }

    #[must_use]
    pub fn snapshot_dir(&self, id: &str) -> PathBuf {
        self.snapshots.current_dir().map_or_else(
            || self.sift_dir.join("snapshots").join(id),
            |d| {
                let parent = d.parent().unwrap_or(&self.sift_dir);
                parent.join(id)
            },
        )
    }

    /// Build a new snapshot using the given index kinds.
    ///
    /// # Errors
    ///
    /// Returns an error if the index build fails, the manifest cannot be
    /// written, or snapshot commit fails.
    pub fn build(
        &mut self,
        kinds: &[super::IndexKind],
        config: &super::IndexBuildConfig<'_>,
    ) -> crate::Result<()> {
        let snapshot = self.snapshots.begin()?;

        for kind in kinds {
            let index_dir = snapshot.dir().join(kind.as_str());
            std::fs::create_dir_all(&index_dir)?;
            kind.build_to_dir(config, &index_dir)?;
        }

        Self::write_manifest(&snapshot, kinds)?;
        self.snapshots.commit(snapshot)?;
        Ok(())
    }

    /// Update the current snapshot, rebuilding only indexes whose corpus
    /// changed.
    ///
    /// Returns `true` if a new snapshot was published, `false` if the corpus
    /// was unchanged.
    ///
    /// # Errors
    ///
    /// Returns an error if the current snapshot cannot be opened, the update
    /// check fails, or publishing the new snapshot fails.
    pub fn update(
        &mut self,
        kinds: &[super::IndexKind],
        config: &super::IndexBuildConfig<'_>,
    ) -> crate::Result<bool> {
        let Some(current_dir) = self.snapshots.current_dir() else {
            self.build(kinds, config)?;
            return Ok(true);
        };

        let snapshot = self.snapshots.begin()?;

        let changed: Vec<bool> = kinds
            .iter()
            .map(|kind| kind.try_update(&current_dir, config, &snapshot.dir().join(kind.as_str())))
            .collect::<crate::Result<_>>()?;

        if !changed.iter().any(|&c| c) {
            return Ok(false);
        }

        for (kind, did_change) in kinds.iter().zip(&changed) {
            if !did_change {
                let src = current_dir.join(kind.as_str());
                if src.exists() {
                    snapshot::copy_dir_contents(&src, &snapshot.dir().join(kind.as_str()))?;
                }
            }
        }

        Self::write_manifest(&snapshot, kinds)?;
        self.snapshots.commit(snapshot)?;
        Ok(true)
    }

    /// Open all indexes in the current snapshot.
    ///
    /// Returns an empty vector if no snapshot exists.
    ///
    /// # Errors
    ///
    /// Returns an error if the manifest is malformed or an index kind is
    /// unknown.
    pub fn open_current(&self) -> crate::Result<Vec<super::Index>> {
        let Some(snapshot_dir) = self.snapshots.current_dir() else {
            return Ok(Vec::new());
        };

        let manifest_path = snapshot_dir.join(MANIFEST_FILE);
        let manifest_raw = std::fs::read_to_string(&manifest_path)?;
        let manifest: SnapshotManifest = serde_json::from_str(&manifest_raw).map_err(|e| {
            crate::Error::Index(IndexError::InvalidManifest {
                path: manifest_path.clone(),
                source: e,
            })
        })?;

        let meta = StoreMeta::read(&self.sift_dir)?;

        let mut indexes = Vec::new();
        for name in &manifest.indexes {
            let kind: super::IndexKind = name
                .parse()
                .map_err(|_| crate::Error::Index(IndexError::UnknownIndexKind(name.clone())))?;
            let index_dir = snapshot_dir.join(name);
            indexes.push(kind.open_from_dir(&index_dir, &meta.root, meta.corpus_kind)?);
        }

        Ok(indexes)
    }

    fn write_manifest(
        snapshot: &super::snapshot::Snapshot,
        kinds: &[super::IndexKind],
    ) -> crate::Result<()> {
        let manifest = SnapshotManifest {
            id: snapshot.id().to_string(),
            indexes: kinds.iter().map(|k| k.as_str().to_string()).collect(),
        };
        let json = serde_json::to_vec_pretty(&manifest).map_err(|e| {
            crate::Error::Index(IndexError::InvalidManifest {
                path: snapshot.dir().join(MANIFEST_FILE),
                source: e,
            })
        })?;
        std::fs::write(snapshot.dir().join(MANIFEST_FILE), json)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::{CorpusKind, IndexBuildConfig, IndexKind};
    use crate::search::filter::VisibilityConfig;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn build_creates_store_layout() {
        let tmp = TempDir::new().expect("create temp dir");
        let corpus = tmp.path().join("corpus");
        fs::create_dir_all(&corpus).expect("create corpus");
        fs::write(corpus.join("f.txt"), "hello world\n").expect("write file");

        let sift_dir = tmp.path().join(".sift");
        let mut store = IndexStore::open_or_create(
            &sift_dir,
            &corpus,
            CorpusKind::Directory,
            false,
            &[IndexKind::Trigram],
        )
        .expect("open store");

        store
            .build(
                &[IndexKind::Trigram],
                &IndexBuildConfig {
                    root: &corpus,
                    follow_links: false,
                    exclude_paths: &[],
                    include_paths: &[],
                    corpus_kind: CorpusKind::Directory,
                    visibility: VisibilityConfig::standard(),
                },
            )
            .expect("build");

        assert!(StoreMeta::path(&sift_dir).exists());

        let id = store.current_id().expect("has current id");
        let snapshot_dir = store.snapshots.current_dir().expect("has snapshot dir");
        assert!(snapshot_dir.exists());
        assert!(snapshot_dir.join(MANIFEST_FILE).exists());
        assert!(snapshot_dir.join("trigram").exists());
        assert!(snapshot_dir.join("trigram").join("files.bin").exists());
        assert!(snapshot_dir.join("trigram").join("lexicon.bin").exists());
        assert!(snapshot_dir.join("trigram").join("postings.bin").exists());
        assert!(snapshot_dir.join("trigram").join("trigrams.bin").exists());

        assert!(!id.is_empty());
    }

    #[test]
    fn open_current_returns_indexes() {
        let tmp = TempDir::new().expect("create temp dir");
        let corpus = tmp.path().join("corpus");
        fs::create_dir_all(&corpus).expect("create corpus");
        fs::write(corpus.join("f.txt"), "hello world\n").expect("write file");

        let sift_dir = tmp.path().join(".sift");
        let mut store = IndexStore::open_or_create(
            &sift_dir,
            &corpus,
            CorpusKind::Directory,
            false,
            &[IndexKind::Trigram],
        )
        .expect("open store");

        store
            .build(
                &[IndexKind::Trigram],
                &IndexBuildConfig {
                    root: &corpus,
                    follow_links: false,
                    exclude_paths: &[],
                    include_paths: &[],
                    corpus_kind: CorpusKind::Directory,
                    visibility: VisibilityConfig::standard(),
                },
            )
            .expect("build");

        drop(store);
        let store = IndexStore::open(&sift_dir).expect("reopen store");
        let indexes = store.open_current().expect("open current");
        assert_eq!(indexes.len(), 1);
        let canon_corpus = corpus.canonicalize().unwrap();
        assert_eq!(indexes[0].root(), &canon_corpus);
    }

    #[test]
    fn open_returns_empty_when_no_current() {
        let tmp = TempDir::new().expect("create temp dir");
        let sift_dir = tmp.path().join(".sift");
        std::fs::create_dir_all(&sift_dir).expect("create sift dir");

        let store = IndexStore::open(&sift_dir).expect("open store");
        assert!(store.current_id().is_none());
        let indexes = store.open_current().expect("open current");
        assert!(indexes.is_empty());
    }

    #[test]
    fn open_without_sift_dir_returns_empty() {
        let tmp = TempDir::new().expect("create temp dir");
        let store = IndexStore::open(&tmp.path().join(".sift")).expect("open store");
        assert!(store.current_id().is_none());
    }

    #[test]
    fn update_skips_rebuild_when_unchanged() {
        let tmp = TempDir::new().expect("create temp dir");
        let corpus = tmp.path().join("corpus");
        fs::create_dir_all(&corpus).expect("create corpus");
        fs::write(corpus.join("f.txt"), "hello world\n").expect("write file");

        let sift_dir = tmp.path().join(".sift");
        let config = IndexBuildConfig {
            root: &corpus,
            follow_links: false,
            exclude_paths: &[],
            include_paths: &[],
            corpus_kind: CorpusKind::Directory,
            visibility: VisibilityConfig::standard(),
        };

        let mut store = IndexStore::open_or_create(
            &sift_dir,
            &corpus,
            CorpusKind::Directory,
            false,
            &[IndexKind::Trigram],
        )
        .expect("open store");
        store.build(&[IndexKind::Trigram], &config).expect("build");

        let id_after_build = store.current_id().expect("has id").to_string();

        let changed = store
            .update(&[IndexKind::Trigram], &config)
            .expect("update");
        assert!(!changed, "expected no rebuild when corpus unchanged");
        assert_eq!(store.current_id().unwrap(), id_after_build);
    }

    #[test]
    fn update_rebuilds_when_file_added() {
        let tmp = TempDir::new().expect("create temp dir");
        let corpus = tmp.path().join("corpus");
        fs::create_dir_all(&corpus).expect("create corpus");
        fs::write(corpus.join("f.txt"), "hello world\n").expect("write file");

        let sift_dir = tmp.path().join(".sift");
        let config = IndexBuildConfig {
            root: &corpus,
            follow_links: false,
            exclude_paths: &[],
            include_paths: &[],
            corpus_kind: CorpusKind::Directory,
            visibility: VisibilityConfig::standard(),
        };

        let mut store = IndexStore::open_or_create(
            &sift_dir,
            &corpus,
            CorpusKind::Directory,
            false,
            &[IndexKind::Trigram],
        )
        .expect("open store");
        store.build(&[IndexKind::Trigram], &config).expect("build");

        let id_after_build = store.current_id().expect("has id").to_string();

        fs::write(corpus.join("g.txt"), "new file\n").expect("write new file");

        let changed = store
            .update(&[IndexKind::Trigram], &config)
            .expect("update");
        assert!(changed, "expected rebuild when file added");
        assert_ne!(store.current_id().unwrap(), id_after_build);
    }

    #[test]
    fn update_builds_when_no_current_snapshot() {
        let tmp = TempDir::new().expect("create temp dir");
        let corpus = tmp.path().join("corpus");
        fs::create_dir_all(&corpus).expect("create corpus");
        fs::write(corpus.join("f.txt"), "hello\n").expect("write file");

        let sift_dir = tmp.path().join(".sift");
        let config = IndexBuildConfig {
            root: &corpus,
            follow_links: false,
            exclude_paths: &[],
            include_paths: &[],
            corpus_kind: CorpusKind::Directory,
            visibility: VisibilityConfig::standard(),
        };

        let mut store = IndexStore::open_or_create(
            &sift_dir,
            &corpus,
            CorpusKind::Directory,
            false,
            &[IndexKind::Trigram],
        )
        .expect("open store");

        let changed = store
            .update(&[IndexKind::Trigram], &config)
            .expect("update");
        assert!(changed, "expected build when no snapshot exists");
        assert!(store.current_id().is_some());
    }
}
