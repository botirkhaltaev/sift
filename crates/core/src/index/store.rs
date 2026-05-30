use std::path::{Path, PathBuf};

use super::snapshot::{
    OnDiskSnapshotStore, SnapshotId, SnapshotLease, SnapshotManifest, SnapshotStore,
    SnapshotWrite, SnapshotWriterSession,
};
use super::{CorpusKind, IndexError};

const MANIFEST_FILE: &str = "manifest.json";

/// Index lifecycle orchestrator backed by a [`SnapshotStore`] for atomic
/// persistence and coordination.
pub struct IndexStore<S: SnapshotStore = OnDiskSnapshotStore> {
    snapshots: S,
    sift_dir: PathBuf,
    meta: Option<super::meta::StoreMeta>,
}

impl IndexStore<OnDiskSnapshotStore> {
    /// Open an existing store at `sift_dir`.
    ///
    /// # Errors
    ///
    /// Returns an error if `CURRENT` exists but cannot be read.
    pub fn open(sift_dir: &Path) -> crate::Result<Self> {
        let snapshots = OnDiskSnapshotStore::open(sift_dir)?;
        let meta = super::meta::StoreMeta::read(sift_dir).ok();
        Ok(Self {
            snapshots,
            sift_dir: sift_dir.to_path_buf(),
            meta,
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

        if !super::meta::StoreMeta::path(sift_dir).exists() {
            let _guard = acquire_write_lock(sift_dir)?;
            if !super::meta::StoreMeta::path(sift_dir).exists() {
                let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
                let meta = super::meta::StoreMeta::new(
                    canonical_root,
                    corpus_kind,
                    follow_links,
                    indexes.to_vec(),
                );
                meta.write(sift_dir)?;
            }
        }

        Self::open(sift_dir)
    }

    #[must_use]
    pub fn snapshot_dir(&self, id: &str) -> PathBuf {
        self.sift_dir.join("snapshots").join(id)
    }

    // ------------------------------------------------------------------
    // Write path
    // ------------------------------------------------------------------

    /// Build a new snapshot using the given index kinds.
    ///
    /// Acquires the writer lock, reloads snapshot state, then builds and
    /// publishes the snapshot.
    ///
    /// Returns the snapshot id.
    ///
    /// # Errors
    ///
    /// Returns an error if the write lock cannot be acquired, the index build
    /// fails, the manifest cannot be written, or snapshot commit fails.
    pub fn build(
        &mut self,
        kinds: &[super::IndexKind],
        config: &super::IndexConfig<'_>,
    ) -> crate::Result<String> {
        let mut writer = self.snapshots.writer()?;
        let txn = writer.begin()?;

        for kind in kinds {
            let index_dir = txn.dir().join(kind.as_str());
            std::fs::create_dir_all(&index_dir)?;
            kind.build(config, &index_dir)?;
        }

        let manifest = SnapshotManifest {
            id: txn.id().clone(),
            indexes: kinds.iter().map(|k| k.as_str().to_string()).collect(),
        };
        let id = writer.publish(txn, manifest)?;
        Ok(id.to_string())
    }

    /// Update the current snapshot, rebuilding only indexes whose corpus
    /// changed.
    ///
    /// Acquires the writer lock, reloads snapshot state, then checks for
    /// changes and publishes a new snapshot if needed.
    ///
    /// Returns the snapshot id if a new snapshot was published, or `None` if
    /// no index changed.
    ///
    /// # Errors
    ///
    /// Returns an error if the write lock cannot be acquired, the current
    /// snapshot cannot be opened, the update check fails, or publishing fails.
    pub fn update(
        &mut self,
        kinds: &[super::IndexKind],
        config: &super::IndexConfig<'_>,
    ) -> crate::Result<Option<String>> {
        let mut writer = self.snapshots.writer()?;

        let Some(current) = writer.current()? else {
            // No current snapshot — build from scratch.
            let txn = writer.begin()?;
            for kind in kinds {
                let index_dir = txn.dir().join(kind.as_str());
                std::fs::create_dir_all(&index_dir)?;
                kind.build(config, &index_dir)?;
            }
            let manifest = SnapshotManifest {
                id: txn.id().clone(),
                indexes: kinds.iter().map(|k| k.as_str().to_string()).collect(),
            };
            let id = writer.publish(txn, manifest)?;
            return Ok(Some(id.to_string()));
        };

        let current_dir = current.dir().to_path_buf();
        let txn = writer.begin()?;

        let changed: Vec<bool> = kinds
            .iter()
            .map(|kind| kind.update(&current_dir, config, &txn.dir().join(kind.as_str())))
            .collect::<crate::Result<_>>()?;

        if !changed.iter().any(|&c| c) {
            return Ok(None);
        }

        for (kind, did_change) in kinds.iter().zip(&changed) {
            if !did_change {
                let src = current_dir.join(kind.as_str());
                if src.exists() {
                    OnDiskSnapshotStore::copy_dir(&src, &txn.dir().join(kind.as_str()))?;
                }
            }
        }

        let manifest = SnapshotManifest {
            id: txn.id().clone(),
            indexes: kinds.iter().map(|k| k.as_str().to_string()).collect(),
        };
        let id = writer.publish(txn, manifest)?;
        Ok(Some(id.to_string()))
    }

    // ------------------------------------------------------------------
    // Read path
    // ------------------------------------------------------------------

    /// Open the current snapshot, returning an immutable [`Snapshot`] with
    /// its indexes opened and a reader lease held.
    ///
    /// Re-reads `CURRENT` from disk to ensure freshness. Retries once if
    /// the snapshot disappears during opening.
    ///
    /// # Errors
    ///
    /// Returns an error if the manifest is malformed, an index kind is
    /// unknown, or the snapshot could not be opened after retry.
    pub(crate) fn open_current(&self) -> crate::Result<super::snapshot::Snapshot> {
        for attempt in 0..2 {
            let Some(current_id) = OnDiskSnapshotStore::read_current_id(&self.sift_dir)? else {
                return Ok(super::snapshot::Snapshot::empty(PathBuf::new()));
            };

            let meta = match self.meta {
                Some(ref m) => m,
                None => {
                    return Err(crate::Error::Index(IndexError::Io {
                        path: self.sift_dir.join("sift.meta"),
                        source: std::io::Error::new(
                            std::io::ErrorKind::NotFound,
                            "store metadata not found",
                        ),
                    }));
                }
            };
            let root = meta.root.clone();
            let corpus_kind = meta.corpus_kind;

            let snap_dir = self.sift_dir.join("snapshots").join(&current_id);

            let lease = SnapshotLease::create_file(&self.sift_dir, &current_id)?;

            if !snap_dir.exists() {
                drop(lease);
                continue;
            }

            let manifest_path = snap_dir.join("manifest.json");
            let manifest_raw = match std::fs::read_to_string(&manifest_path) {
                Ok(raw) => raw,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound && attempt == 0 => {
                    drop(lease);
                    continue;
                }
                Err(e) => return Err(crate::Error::Io(e)),
            };

            let manifest: SnapshotManifest =
                serde_json::from_str(&manifest_raw).map_err(|e| {
                    crate::Error::Index(IndexError::InvalidManifest {
                        path: manifest_path.clone(),
                        source: e,
                    })
                })?;

            let mut indexes = Vec::new();
            for name in &manifest.indexes {
                let kind: super::IndexKind = name.parse().map_err(|_| {
                    crate::Error::Index(IndexError::UnknownIndexKind(name.clone()))
                })?;
                let index_dir = snap_dir.join(name);
                indexes.push(kind.open_from_dir(&index_dir, &root, corpus_kind)?);
            }

            return Ok(super::snapshot::Snapshot::current(root, indexes, lease));
        }

        Err(crate::Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "snapshot disappeared during open",
        )))
    }

    // ------------------------------------------------------------------
    // Internals
    // ------------------------------------------------------------------

    /// Acquire the exclusive writer lock for this store.
    ///
    /// The lock lives at `<sift_dir>/write.lock` and is released when the
    /// returned guard is dropped.
    fn acquire_write_lock(&self) -> crate::Result<WriteLockGuard> {
        acquire_write_lock(&self.sift_dir)
    }
}

impl<S: SnapshotStore> IndexStore<S> {
    #[must_use]
    pub fn current_id(&self) -> Option<&str> {
        self.snapshots.current_id().map(SnapshotId::as_str)
    }
}
fn acquire_write_lock(sift_dir: &Path) -> crate::Result<WriteLockGuard> {
    let lock_path = sift_dir.join("write.lock");
    let mut lock_file = fslock::LockFile::open(&lock_path)?;
    lock_file.lock()?;
    Ok(WriteLockGuard { _file: lock_file })
}

/// Guard that releases the write lock when dropped.
struct WriteLockGuard {
    _file: fslock::LockFile,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::meta::StoreMeta;
    use crate::index::{CorpusKind, CorpusSpec, IndexConfig, IndexKind};
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
                &IndexConfig {
                    corpus: CorpusSpec {
                        root: &corpus,
                        kind: CorpusKind::Directory,
                        follow_links: false,
                        include_paths: &[],
                        exclude_paths: &[],
                    },
                    visibility: VisibilityConfig::default(),
                },
            )
            .expect("build");

        assert!(StoreMeta::path(&sift_dir).exists());

        let id = store.current_id().expect("has current id");
        let snapshot_dir = store.snapshot_dir(&id);
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
                &IndexConfig {
                    corpus: CorpusSpec {
                        root: &corpus,
                        kind: CorpusKind::Directory,
                        follow_links: false,
                        include_paths: &[],
                        exclude_paths: &[],
                    },
                    visibility: VisibilityConfig::default(),
                },
            )
            .expect("build");

        drop(store);
        let store = IndexStore::open(&sift_dir).expect("reopen store");
        let snapshot = store.open_current().expect("open current");
        assert_eq!(snapshot.indexes().len(), 1);
        let canon_corpus = corpus.canonicalize().unwrap();
        assert_eq!(snapshot.indexes()[0].root(), &canon_corpus);
        assert!(store.current_id().is_some(), "store should have current id");
    }

    #[test]
    fn open_returns_empty_when_no_current() {
        let tmp = TempDir::new().expect("create temp dir");
        let sift_dir = tmp.path().join(".sift");
        std::fs::create_dir_all(&sift_dir).expect("create sift dir");

        let store = IndexStore::open(&sift_dir).expect("open store");
        assert!(store.current_id().is_none());
        let snapshot = store.open_current().expect("open current");
        assert!(snapshot.is_empty());
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
        let config = IndexConfig {
            corpus: CorpusSpec {
                root: &corpus,
                kind: CorpusKind::Directory,
                follow_links: false,
                include_paths: &[],
                exclude_paths: &[],
            },
            visibility: VisibilityConfig::default(),
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
        assert_eq!(changed, None, "expected no rebuild when corpus unchanged");
        assert_eq!(store.current_id().unwrap(), id_after_build);
    }

    #[test]
    fn update_rebuilds_when_file_added() {
        let tmp = TempDir::new().expect("create temp dir");
        let corpus = tmp.path().join("corpus");
        fs::create_dir_all(&corpus).expect("create corpus");
        fs::write(corpus.join("f.txt"), "hello world\n").expect("write file");

        let sift_dir = tmp.path().join(".sift");
        let config = IndexConfig {
            corpus: CorpusSpec {
                root: &corpus,
                kind: CorpusKind::Directory,
                follow_links: false,
                include_paths: &[],
                exclude_paths: &[],
            },
            visibility: VisibilityConfig::default(),
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
        assert!(changed.is_some(), "expected rebuild when file added");
        assert_ne!(store.current_id().unwrap(), id_after_build);
    }

    #[test]
    fn update_builds_when_no_current_snapshot() {
        let tmp = TempDir::new().expect("create temp dir");
        let corpus = tmp.path().join("corpus");
        fs::create_dir_all(&corpus).expect("create corpus");
        fs::write(corpus.join("f.txt"), "hello\n").expect("write file");

        let sift_dir = tmp.path().join(".sift");
        let config = IndexConfig {
            corpus: CorpusSpec {
                root: &corpus,
                kind: CorpusKind::Directory,
                follow_links: false,
                include_paths: &[],
                exclude_paths: &[],
            },
            visibility: VisibilityConfig::default(),
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
        assert!(changed.is_some(), "expected build when no snapshot exists");
        assert!(store.current_id().is_some());
    }

    #[test]
    fn build_acquires_write_lock() {
        let tmp = TempDir::new().expect("create temp dir");
        let corpus = tmp.path().join("corpus");
        fs::create_dir_all(&corpus).expect("create corpus");
        fs::write(corpus.join("f.txt"), "content\n").expect("write file");

        let sift_dir = tmp.path().join(".sift");
        let config = IndexConfig {
            corpus: CorpusSpec {
                root: &corpus,
                kind: CorpusKind::Directory,
                follow_links: false,
                include_paths: &[],
                exclude_paths: &[],
            },
            visibility: VisibilityConfig::default(),
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

        // Verify lock was released after build by acquiring it externally.
        let lock_path = sift_dir.join("write.lock");
        let mut lock = fslock::LockFile::open(&lock_path).expect("open lock");
        assert!(
            lock.try_lock().expect("try lock"),
            "write lock should be released after build"
        );
        drop(lock);
    }

    #[test]
    fn writer_refreshes_snapshot_state_on_acquire() {
        let tmp = TempDir::new().expect("create temp dir");
        let corpus = tmp.path().join("corpus");
        fs::create_dir_all(&corpus).expect("create corpus");
        fs::write(corpus.join("f.txt"), "hello\n").expect("write file");

        let sift_dir = tmp.path().join(".sift");
        let config = IndexConfig {
            corpus: CorpusSpec {
                root: &corpus,
                kind: CorpusKind::Directory,
                follow_links: false,
                include_paths: &[],
                exclude_paths: &[],
            },
            visibility: VisibilityConfig::default(),
        };

        // Store A builds.
        let mut store_a = IndexStore::open_or_create(
            &sift_dir,
            &corpus,
            CorpusKind::Directory,
            false,
            &[IndexKind::Trigram],
        )
        .expect("open store");
        store_a
            .build(&[IndexKind::Trigram], &config)
            .expect("build");

        // Store A publishes a new snapshot.
        fs::write(corpus.join("g.txt"), "new\n").expect("write");
        store_a
            .update(&[IndexKind::Trigram], &config)
            .expect("update");

        // Store B opens — it should see the same current (reads fresh from disk
        // at open time, then caches).
        let mut store_b = IndexStore::open(&sift_dir).expect("open store");
        assert_eq!(
            store_b.current_id(),
            store_a.current_id(),
            "freshly-opened store_b should see store_a's latest snapshot"
        );

        // Store B acquires the write lock for its own update, which refreshes
        // its snapshot state.  Since the corpus is unchanged, no new snapshot
        // is published and current_id stays the same.
        let changed = store_b
            .update(&[IndexKind::Trigram], &config)
            .expect("update");
        assert_eq!(changed, None, "corpus unchanged after store_a update");
        assert_eq!(
            store_b.current_id(),
            store_a.current_id(),
            "store_b should still point to the same snapshot"
        );
    }
}
