use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use super::{CorpusKind, Index, IndexBuildConfig, IndexError};

const STORE_VERSION: u32 = 1;
const SNAPSHOTS_DIR: &str = "snapshots";
const CURRENT_FILE: &str = "CURRENT";
const META_FILE: &str = "meta.json";
const MANIFEST_FILE: &str = "manifest.json";

fn snapshot_id(counter: &AtomicU64) -> String {
    format!("{:016x}", counter.fetch_add(1, Ordering::Relaxed))
}

fn read_current(path: &Path) -> crate::Result<String> {
    let raw = std::fs::read_to_string(path)?;
    Ok(raw.trim().to_string())
}

fn write_atomic(path: &Path, contents: &str) -> crate::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreMeta {
    pub version: u32,
    pub root: PathBuf,
    pub corpus_kind: CorpusKind,
    pub follow_links: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct SnapshotManifest {
    id: String,
    root: PathBuf,
    corpus_kind: CorpusKind,
    indexes: Vec<String>,
}

pub struct IndexStore {
    sift_dir: PathBuf,
    counter: AtomicU64,
    current_id: Option<String>,
}

impl IndexStore {
    /// Open an existing store at `sift_dir`.
    ///
    /// # Errors
    ///
    /// Returns an error if `CURRENT` exists but cannot be read, or if stale
    /// snapshot cleanup fails.
    pub fn open(sift_dir: &Path) -> crate::Result<Self> {
        let current_path = sift_dir.join(CURRENT_FILE);
        let current_id = if current_path.exists() {
            Some(read_current(&current_path)?)
        } else {
            None
        };

        Self::cleanup_stale(sift_dir, current_id.as_deref())?;

        Ok(Self {
            sift_dir: sift_dir.to_path_buf(),
            counter: AtomicU64::new(1),
            current_id,
        })
    }

    /// Open an existing store or create a new one at `sift_dir`.
    ///
    /// # Errors
    ///
    /// Returns an error if the store directory cannot be created, metadata
    /// cannot be written, or stale snapshot cleanup fails.
    pub fn open_or_create(
        sift_dir: &Path,
        root: &Path,
        corpus_kind: CorpusKind,
        follow_links: bool,
    ) -> crate::Result<Self> {
        std::fs::create_dir_all(sift_dir)?;

        let meta_path = sift_dir.join(META_FILE);
        if !meta_path.exists() {
            let store_meta = StoreMeta {
                version: STORE_VERSION,
                root: root.to_path_buf(),
                corpus_kind,
                follow_links,
            };
            let json = serde_json::to_vec_pretty(&store_meta).map_err(|e| {
                crate::Error::Index(crate::index::IndexError::InvalidManifest {
                    path: meta_path.clone(),
                    source: e,
                })
            })?;
            std::fs::write(&meta_path, json)?;
        }

        let current_path = sift_dir.join(CURRENT_FILE);
        let current_id = if current_path.exists() {
            Some(read_current(&current_path)?)
        } else {
            None
        };

        Self::cleanup_stale(sift_dir, current_id.as_deref())?;

        Ok(Self {
            sift_dir: sift_dir.to_path_buf(),
            counter: AtomicU64::new(1),
            current_id,
        })
    }

    pub fn current_id(&self) -> Option<&str> {
        self.current_id.as_deref()
    }

    pub fn snapshot_dir(&self, id: &str) -> PathBuf {
        self.sift_dir.join(SNAPSHOTS_DIR).join(id)
    }

    /// Build a new snapshot using the given index implementation.
    ///
    /// # Errors
    ///
    /// Returns an error if the index build fails, the manifest cannot be
    /// written, or snapshot rename/publish fails.
    pub fn build<I: Index>(&mut self, config: &IndexBuildConfig<'_>) -> crate::Result<I> {
        let snapshots_dir = self.sift_dir.join(SNAPSHOTS_DIR);
        std::fs::create_dir_all(&snapshots_dir)?;

        let id = snapshot_id(&self.counter);
        let tmp_dir = snapshots_dir.join(format!("tmp-{id}"));
        let index_dir = tmp_dir.join(I::kind_name());
        std::fs::create_dir_all(&index_dir)?;

        let index = I::build(config, &index_dir)?;

        let canonical_root = config
            .root
            .canonicalize()
            .unwrap_or_else(|_| config.root.to_path_buf());
        let manifest = SnapshotManifest {
            id: id.clone(),
            root: canonical_root,
            corpus_kind: config.corpus_kind,
            indexes: vec![I::kind_name().to_string()],
        };
        let tmp_manifest = tmp_dir.join(MANIFEST_FILE);
        let json = serde_json::to_vec_pretty(&manifest).map_err(|e| {
            crate::Error::Index(crate::index::IndexError::InvalidManifest {
                path: tmp_manifest.clone(),
                source: e,
            })
        })?;
        std::fs::write(tmp_dir.join(MANIFEST_FILE), json)?;

        let final_dir = snapshots_dir.join(&id);
        std::fs::rename(&tmp_dir, &final_dir)?;

        let current_path = self.sift_dir.join(CURRENT_FILE);
        write_atomic(&current_path, &id)?;

        let old_current = self.current_id.replace(id);
        if let Some(ref old_id) = old_current {
            let old_dir = snapshots_dir.join(old_id);
            if old_dir.exists() {
                std::fs::remove_dir_all(&old_dir)?;
            }
        }

        Ok(index)
    }

    /// Open all indexes in the current snapshot.
    ///
    /// Returns an empty vector if no snapshot exists.
    ///
    /// # Errors
    ///
    /// Returns an error if the manifest is malformed or an index kind is unknown.
    pub fn open_current(&self) -> crate::Result<Vec<Box<dyn Index>>> {
        let Some(id) = &self.current_id else {
            return Ok(Vec::new());
        };

        let snapshot_dir = self.sift_dir.join(SNAPSHOTS_DIR).join(id);
        let manifest_path = snapshot_dir.join(MANIFEST_FILE);
        let manifest_raw = std::fs::read_to_string(&manifest_path)?;
        let manifest: SnapshotManifest = serde_json::from_str(&manifest_raw).map_err(|e| {
            crate::Error::Index(IndexError::InvalidManifest {
                path: manifest_path.clone(),
                source: e,
            })
        })?;

        let root = &manifest.root;
        let corpus_kind = manifest.corpus_kind;

        let mut indexes: Vec<Box<dyn Index>> = Vec::new();
        for name in &manifest.indexes {
            let index_dir = snapshot_dir.join(name);
            match name.as_str() {
                "trigram" => {
                    let idx =
                        crate::index::trigram::TrigramIndex::open(&index_dir, root, corpus_kind)?;
                    indexes.push(Box::new(idx));
                }
                other => {
                    return Err(crate::Error::Index(IndexError::UnknownIndexKind(
                        other.to_string(),
                    )));
                }
            }
        }

        Ok(indexes)
    }

    /// Read store metadata from disk.
    ///
    /// # Errors
    ///
    /// Returns an error if `meta.json` is missing or malformed.
    pub fn read_meta(sift_dir: &Path) -> crate::Result<StoreMeta> {
        let meta_path = sift_dir.join(META_FILE);
        let raw = std::fs::read_to_string(&meta_path)?;
        serde_json::from_str(&raw).map_err(|e| {
            crate::Error::Index(crate::index::IndexError::InvalidManifest {
                path: meta_path,
                source: e,
            })
        })
    }

    /// Root directory of the indexed corpus.
    ///
    /// # Errors
    ///
    /// Returns an error if `meta.json` is missing or malformed.
    pub fn meta_root(&self) -> crate::Result<PathBuf> {
        Self::read_meta(&self.sift_dir).map(|m| m.root)
    }

    /// Which kind of corpus was indexed.
    ///
    /// # Errors
    ///
    /// Returns an error if `meta.json` is missing or malformed.
    pub fn meta_corpus_kind(&self) -> crate::Result<CorpusKind> {
        Self::read_meta(&self.sift_dir).map(|m| m.corpus_kind)
    }

    /// Whether symlinks were followed during the build.
    ///
    /// # Errors
    ///
    /// Returns an error if `meta.json` is missing or malformed.
    pub fn meta_follow_links(&self) -> crate::Result<bool> {
        Self::read_meta(&self.sift_dir).map(|m| m.follow_links)
    }

    fn cleanup_stale(sift_dir: &Path, current_id: Option<&str>) -> crate::Result<()> {
        let snapshots_dir = sift_dir.join(SNAPSHOTS_DIR);
        let Ok(entries) = std::fs::read_dir(&snapshots_dir) else {
            return Ok(());
        };

        for entry in entries {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy().into_owned();
            if name_str.starts_with("tmp-") {
                std::fs::remove_dir_all(entry.path())?;
            } else if let Some(current) = current_id
                && name_str != current
            {
                std::fs::remove_dir_all(entry.path())?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::trigram::TrigramIndex;
    use crate::index::{CorpusKind, IndexBuildConfig};
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn build_creates_store_layout() {
        let tmp = TempDir::new().expect("create temp dir");
        let corpus = tmp.path().join("corpus");
        fs::create_dir_all(&corpus).expect("create corpus");
        fs::write(corpus.join("f.txt"), "hello world\n").expect("write file");

        let sift_dir = tmp.path().join(".sift");
        let mut store =
            IndexStore::open_or_create(&sift_dir, &corpus, CorpusKind::Directory, false)
                .expect("open store");

        let _: TrigramIndex = store
            .build::<TrigramIndex>(&IndexBuildConfig {
                root: &corpus,
                follow_links: false,
                exclude_paths: &[],
                include_paths: &[],
                corpus_kind: CorpusKind::Directory,
            })
            .expect("build");

        assert!(sift_dir.join(META_FILE).exists());
        assert!(sift_dir.join(CURRENT_FILE).exists());

        let id = store.current_id().expect("has current id");
        let snapshot_dir = store.snapshot_dir(id);
        assert!(snapshot_dir.exists());
        assert!(snapshot_dir.join(MANIFEST_FILE).exists());
        assert!(snapshot_dir.join("trigram").exists());
        assert!(snapshot_dir.join("trigram").join("files.bin").exists());
        assert!(snapshot_dir.join("trigram").join("lexicon.bin").exists());
        assert!(snapshot_dir.join("trigram").join("postings.bin").exists());
    }

    #[test]
    fn open_current_returns_indexes() {
        let tmp = TempDir::new().expect("create temp dir");
        let corpus = tmp.path().join("corpus");
        fs::create_dir_all(&corpus).expect("create corpus");
        fs::write(corpus.join("f.txt"), "hello world\n").expect("write file");

        let sift_dir = tmp.path().join(".sift");
        let mut store =
            IndexStore::open_or_create(&sift_dir, &corpus, CorpusKind::Directory, false)
                .expect("open store");

        let _: TrigramIndex = store
            .build::<TrigramIndex>(&IndexBuildConfig {
                root: &corpus,
                follow_links: false,
                exclude_paths: &[],
                include_paths: &[],
                corpus_kind: CorpusKind::Directory,
            })
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
    fn cleanup_removes_stale_temp_dirs() {
        let tmp = TempDir::new().expect("create temp dir");
        let snapshots_dir = tmp.path().join(".sift").join(SNAPSHOTS_DIR);
        fs::create_dir_all(&snapshots_dir).expect("create snapshots dir");
        fs::create_dir_all(snapshots_dir.join("tmp-stale")).expect("create stale tmp");
        fs::create_dir_all(snapshots_dir.join("0000000000000001")).expect("create snapshot");

        let current = "0000000000000001";
        IndexStore::cleanup_stale(tmp.path().join(".sift").as_path(), Some(current))
            .expect("cleanup");

        assert!(!snapshots_dir.join("tmp-stale").exists());
        assert!(snapshots_dir.join("0000000000000001").exists());
    }

    #[test]
    fn open_without_sift_dir_returns_empty() {
        let tmp = TempDir::new().expect("create temp dir");
        let store = IndexStore::open(&tmp.path().join(".sift")).expect("open store");
        assert!(store.current_id().is_none());
    }
}
