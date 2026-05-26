use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{CorpusKind, IndexBuildConfig, IndexError};

const STORE_VERSION: u32 = 1;
const SNAPSHOTS_DIR: &str = "snapshots";
const CURRENT_FILE: &str = "CURRENT";
const META_FILE: &str = "meta.json";
const MANIFEST_FILE: &str = "manifest.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreMeta {
    pub version: u32,
    pub root: PathBuf,
    pub corpus_kind: CorpusKind,
    pub follow_links: bool,
    #[serde(default)]
    pub indexes: Vec<super::IndexKind>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SnapshotManifest {
    id: String,
    indexes: Vec<String>,
}

pub struct IndexStore {
    sift_dir: PathBuf,
    current_id: Option<String>,
}

impl IndexStore {
    fn snapshot_id() -> String {
        let d = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        format!("{:010x}-{:08x}", d.as_secs(), d.subsec_nanos())
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

    /// Open an existing store at `sift_dir`.
    ///
    /// # Errors
    ///
    /// Returns an error if `CURRENT` exists but cannot be read.
    pub fn open(sift_dir: &Path) -> crate::Result<Self> {
        let current_path = sift_dir.join(CURRENT_FILE);
        let current_id = if current_path.exists() {
            Some(Self::read_current(&current_path)?)
        } else {
            None
        };

        Ok(Self {
            sift_dir: sift_dir.to_path_buf(),
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
        indexes: &[super::IndexKind],
    ) -> crate::Result<Self> {
        std::fs::create_dir_all(sift_dir)?;

        let meta_path = sift_dir.join(META_FILE);
        if !meta_path.exists() {
            let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
            let store_meta = StoreMeta {
                version: STORE_VERSION,
                root: canonical_root,
                corpus_kind,
                follow_links,
                indexes: indexes.to_vec(),
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
            Some(Self::read_current(&current_path)?)
        } else {
            None
        };

        Ok(Self {
            sift_dir: sift_dir.to_path_buf(),
            current_id,
        })
    }

    #[must_use]
    pub fn current_id(&self) -> Option<&str> {
        self.current_id.as_deref()
    }

    #[must_use]
    pub fn snapshot_dir(&self, id: &str) -> PathBuf {
        self.sift_dir.join(SNAPSHOTS_DIR).join(id)
    }

    /// Build a new snapshot using the given index kinds.
    ///
    /// # Errors
    ///
    /// Returns an error if the index build fails, the manifest cannot be
    /// written, or snapshot rename/publish fails.
    pub fn build(
        &mut self,
        kinds: &[super::IndexKind],
        config: &IndexBuildConfig<'_>,
    ) -> crate::Result<()> {
        let snapshots_dir = self.sift_dir.join(SNAPSHOTS_DIR);
        std::fs::create_dir_all(&snapshots_dir)?;

        let id = Self::snapshot_id();
        let tmp_dir = snapshots_dir.join(format!("tmp-{id}"));

        for kind in kinds {
            let index_dir = tmp_dir.join(kind.as_str());
            std::fs::create_dir_all(&index_dir)?;
            kind.build_to_dir(config, &index_dir)?;
        }

        let index_names: Vec<String> = kinds.iter().map(|k| k.as_str().to_string()).collect();
        self.publish(&snapshots_dir, &id, &tmp_dir, &index_names)?;
        Ok(())
    }

    /// Update the current snapshot, rebuilding only if the corpus changed.
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
        config: &IndexBuildConfig<'_>,
    ) -> crate::Result<bool> {
        let Some(current) = &self.current_id else {
            self.build(kinds, config)?;
            return Ok(true);
        };

        let snapshots_dir = self.sift_dir.join(SNAPSHOTS_DIR);
        let current_snapshot = snapshots_dir.join(current);

        let id = Self::snapshot_id();
        let tmp_dir = snapshots_dir.join(format!("tmp-{id}"));

        let mut any_changed = false;
        for kind in kinds {
            let index_dir = tmp_dir.join(kind.as_str());
            let changed = kind.try_update(&current_snapshot, config, &index_dir)?;
            if changed {
                any_changed = true;
            } else {
                let src = current_snapshot.join(kind.as_str());
                if src.exists() {
                    copy_dir_contents(&src, &index_dir)?;
                }
            }
        }

        if !any_changed {
            let _ = std::fs::remove_dir_all(&tmp_dir);
            return Ok(false);
        }

        let index_names: Vec<String> = kinds.iter().map(|k| k.as_str().to_string()).collect();
        self.publish(&snapshots_dir, &id, &tmp_dir, &index_names)?;
        Ok(true)
    }

    fn publish(
        &mut self,
        snapshots_dir: &Path,
        id: &str,
        tmp_dir: &Path,
        index_names: &[String],
    ) -> crate::Result<()> {
        let manifest = SnapshotManifest {
            id: id.to_string(),
            indexes: index_names.to_vec(),
        };
        let json = serde_json::to_vec_pretty(&manifest).map_err(|e| {
            crate::Error::Index(crate::index::IndexError::InvalidManifest {
                path: tmp_dir.join(MANIFEST_FILE),
                source: e,
            })
        })?;
        std::fs::write(tmp_dir.join(MANIFEST_FILE), json)?;

        let final_dir = snapshots_dir.join(id);
        std::fs::rename(tmp_dir, &final_dir)?;

        let current_path = self.sift_dir.join(CURRENT_FILE);
        Self::write_atomic(&current_path, id)?;

        let old_current = self.current_id.replace(id.to_string());
        let mut keep: Vec<&str> = vec![id];
        if let Some(ref old_id) = old_current {
            keep.push(old_id.as_str());
        }
        gc_snapshots(snapshots_dir, &keep)?;

        Ok(())
    }

    /// Open all indexes in the current snapshot.
    ///
    /// Returns an empty vector if no snapshot exists.
    ///
    /// # Errors
    ///
    /// Returns an error if the manifest is malformed or an index kind is unknown.
    pub fn open_current(&self) -> crate::Result<Vec<super::Index>> {
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

        let meta = Self::read_meta(&self.sift_dir)?;

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
}

fn gc_snapshots(snapshots_dir: &Path, keep: &[&str]) -> crate::Result<()> {
    let Ok(entries) = std::fs::read_dir(snapshots_dir) else {
        return Ok(());
    };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with("tmp-") || !keep.iter().any(|k| *k == name_str.as_ref()) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
    Ok(())
}

fn copy_dir_contents(src: &Path, dst: &Path) -> crate::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        let dest_path = dst.join(entry.file_name());
        if ft.is_dir() {
            copy_dir_contents(&entry.path(), &dest_path)?;
        } else {
            std::fs::copy(entry.path(), &dest_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::{CorpusKind, IndexBuildConfig, IndexKind};
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
                },
            )
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
        assert!(snapshot_dir.join("trigram").join("trigrams.bin").exists());
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
    fn gc_removes_stale_temp_dirs() {
        let tmp = TempDir::new().expect("create temp dir");
        let snapshots_dir = tmp.path().join(".sift").join(SNAPSHOTS_DIR);
        fs::create_dir_all(&snapshots_dir).expect("create snapshots dir");
        fs::create_dir_all(snapshots_dir.join("tmp-stale")).expect("create stale tmp");
        fs::create_dir_all(snapshots_dir.join("0000000000000001")).expect("create snapshot");

        gc_snapshots(&snapshots_dir, &["0000000000000001"]).expect("gc");

        assert!(!snapshots_dir.join("tmp-stale").exists());
        assert!(snapshots_dir.join("0000000000000001").exists());
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
