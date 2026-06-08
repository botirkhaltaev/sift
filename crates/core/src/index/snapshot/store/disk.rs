use std::path::{Path, PathBuf};

use super::super::artifact::ArtifactData;
use super::super::identity::SnapshotId;
use super::super::lease::SnapshotLease;
use super::super::manifest::SnapshotManifest;
use super::{SnapshotRead, SnapshotStore, SnapshotWrite, SnapshotWriterSession};

const SNAPSHOTS_DIR: &str = "snapshots";
const CURRENT_FILE: &str = "CURRENT";
const WRITE_LOCK: &str = "write.lock";

/// Disk-backed snapshot store with `snapshots/`, `CURRENT`, `leases/`, and
/// `write.lock`.
pub struct DiskSnapshotStore {
    dir: PathBuf,
    current_id: Option<SnapshotId>,
}

impl DiskSnapshotStore {
    pub fn open(dir: &Path) -> crate::Result<Self> {
        let current_path = dir.join(CURRENT_FILE);
        let current_id = if current_path.exists() {
            Some(SnapshotId::new(Self::read_current(&current_path)?))
        } else {
            None
        };
        Ok(Self {
            dir: dir.to_path_buf(),
            current_id,
        })
    }

    /// Read `CURRENT` from disk afresh.
    pub fn read_current_id(dir: &Path) -> crate::Result<Option<String>> {
        let current_path = dir.join(CURRENT_FILE);
        if current_path.exists() {
            Self::read_current(&current_path).map(Some)
        } else {
            Ok(None)
        }
    }

    pub fn leases_dir(&self) -> PathBuf {
        self.dir.join(super::super::lease::LEASES_DIR)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn snapshots_dir(&self) -> PathBuf {
        self.dir.join(SNAPSHOTS_DIR)
    }

    fn current_path(&self) -> PathBuf {
        self.dir.join(CURRENT_FILE)
    }

    pub(crate) fn read_current(path: &Path) -> crate::Result<String> {
        let raw = std::fs::read_to_string(path)?;
        Ok(raw.trim().to_string())
    }

    fn write_atomic(path: &Path, contents: &str) -> crate::Result<()> {
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, contents)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    fn generate_id() -> String {
        let d = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        format!("{:010x}-{:08x}", d.as_secs(), d.subsec_nanos())
    }

    pub fn active_lease_ids(dir: &Path) -> crate::Result<Vec<String>> {
        let leases_dir = dir.join(super::super::lease::LEASES_DIR);
        let Ok(entries) = std::fs::read_dir(&leases_dir) else {
            return Ok(Vec::new());
        };
        let stale_threshold = std::time::Duration::from_hours(1);
        let mut ids = Vec::new();
        for entry in entries {
            let entry = entry?;
            if !entry.file_type().is_ok_and(|t| t.is_file()) {
                continue;
            }
            if let Ok(metadata) = entry.metadata()
                && let Ok(mtime) = metadata.modified()
                && let Ok(age) = mtime.elapsed()
                && age > stale_threshold
            {
                let _ = std::fs::remove_file(entry.path());
                continue;
            }
            if let Ok(raw) = std::fs::read_to_string(entry.path()) {
                let id = raw.trim().to_string();
                if !id.is_empty() {
                    ids.push(id);
                }
            }
        }
        Ok(ids)
    }

    fn gc(snapshots_dir: &Path, leases_dir: &Path, keep: &[String]) -> crate::Result<()> {
        let Ok(entries) = std::fs::read_dir(snapshots_dir) else {
            return Ok(());
        };
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with("tmp-") {
                let _ = std::fs::remove_dir_all(entry.path());
                continue;
            }
            if keep.iter().any(|k| *k == name_str.as_ref()) {
                continue;
            }
            let store_dir = snapshots_dir.parent().unwrap_or(leases_dir);
            if let Ok(leased) = Self::active_lease_ids(store_dir)
                && leased.iter().any(|id| id == name_str.as_ref())
            {
                continue;
            }
            let _ = std::fs::remove_dir_all(entry.path());
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Writer session
// ---------------------------------------------------------------------------

/// A writer session that holds the write lock for a [`DiskSnapshotStore`].
pub struct DiskSnapshotWriterSession<'a> {
    store: &'a mut DiskSnapshotStore,
    _lock: fslock::LockFile,
}

impl Drop for DiskSnapshotWriterSession<'_> {
    fn drop(&mut self) {
        // Lock is released when `_lock` is dropped.
    }
}

impl SnapshotWriterSession for DiskSnapshotWriterSession<'_> {
    type Read = DiskSnapshotReader;
    type Write = DiskSnapshotWriter;

    fn current_id(&self) -> Option<&SnapshotId> {
        self.store.current_id.as_ref()
    }

    fn current(&self) -> crate::Result<Option<Self::Read>> {
        let Some(ref current_id) = self.store.current_id else {
            return Ok(None);
        };
        let snap_dir = self.store.snapshots_dir().join(current_id.as_str());
        if !snap_dir.exists() {
            return Ok(None);
        }
        let manifest_path = snap_dir.join("manifest.json");
        let raw = match std::fs::read_to_string(&manifest_path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(crate::Error::Io(e)),
        };
        let manifest: SnapshotManifest = serde_json::from_str(&raw).map_err(|e| {
            crate::Error::Index(super::super::super::IndexError::InvalidManifest {
                path: manifest_path,
                source: e,
            })
        })?;
        let lease = SnapshotLease::create_file(&self.store.dir, current_id.as_str())?;
        Ok(Some(DiskSnapshotReader {
            id: current_id.clone(),
            dir: snap_dir,
            manifest,
            _lease: lease,
        }))
    }

    fn begin(&mut self) -> crate::Result<Self::Write> {
        let snapshots_dir = self.store.snapshots_dir();
        std::fs::create_dir_all(&snapshots_dir)?;
        let id_str = DiskSnapshotStore::generate_id();
        let id = SnapshotId::new(id_str);
        let tmp_dir = snapshots_dir.join(format!("tmp-{}", id.as_str()));
        std::fs::create_dir_all(&tmp_dir)?;
        Ok(DiskSnapshotWriter {
            id,
            dir: tmp_dir,
            committed: false,
        })
    }

    fn publish(
        &mut self,
        mut write: Self::Write,
        manifest: SnapshotManifest,
    ) -> crate::Result<SnapshotId> {
        let snapshots_dir = self.store.snapshots_dir();

        // Write manifest into the temp dir before rename.
        let manifest_json = serde_json::to_vec_pretty(&manifest).map_err(|e| {
            crate::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })?;
        std::fs::write(write.dir.join("manifest.json"), manifest_json)?;

        let final_dir = snapshots_dir.join(write.id.as_str());
        std::fs::rename(&write.dir, &final_dir)?;
        write.committed = true;

        let current_path = self.store.current_path();
        DiskSnapshotStore::write_atomic(&current_path, write.id.as_str())?;

        let old_current = self.store.current_id.replace(write.id.clone());
        let mut keep: Vec<String> = vec![write.id.to_string()];
        if let Some(ref old_id) = old_current {
            keep.push(old_id.to_string());
        }

        let leases_dir = self.store.leases_dir();
        DiskSnapshotStore::gc(&snapshots_dir, &leases_dir, &keep)?;

        Ok(write.id.clone())
    }
}

// ---------------------------------------------------------------------------
// Writer
// ---------------------------------------------------------------------------

/// An in-progress snapshot write backed by a temporary directory on disk.
pub struct DiskSnapshotWriter {
    pub(crate) id: SnapshotId,
    pub(crate) dir: PathBuf,
    pub(crate) committed: bool,
}

impl SnapshotWrite for DiskSnapshotWriter {
    fn id(&self) -> &SnapshotId {
        &self.id
    }

    fn put_artifact(&mut self, namespace: &str, name: &str, bytes: Vec<u8>) -> crate::Result<()> {
        let dir = self.dir.join(namespace);
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join(name), &bytes)?;
        Ok(())
    }
}

impl Drop for DiskSnapshotWriter {
    fn drop(&mut self) {
        if !self.committed {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

// ---------------------------------------------------------------------------
// Reader
// ---------------------------------------------------------------------------

/// A readable snapshot backed by on-disk files.
pub struct DiskSnapshotReader {
    id: SnapshotId,
    dir: PathBuf,
    manifest: SnapshotManifest,
    _lease: SnapshotLease,
}

impl SnapshotRead for DiskSnapshotReader {
    fn id(&self) -> &SnapshotId {
        &self.id
    }

    fn manifest(&self) -> &SnapshotManifest {
        &self.manifest
    }

    fn artifact(&self, namespace: &str, name: &str) -> crate::Result<ArtifactData> {
        let path = self.dir.join(namespace).join(name);
        if !path.exists() {
            return Err(crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!(
                    "artifact {namespace}/{name} not found in snapshot {}",
                    self.id
                ),
            )));
        }
        let mmap = crate::index::trigram::storage::mmap::mmap_open(&path)?;
        Ok(ArtifactData::Mmap(mmap))
    }
}

// ---------------------------------------------------------------------------
// SnapshotStore impl
// ---------------------------------------------------------------------------

impl SnapshotStore for DiskSnapshotStore {
    type Read = DiskSnapshotReader;
    type Write = DiskSnapshotWriter;
    type Writer<'a> = DiskSnapshotWriterSession<'a>;

    fn current_id(&self) -> Option<&SnapshotId> {
        self.current_id.as_ref()
    }

    fn current(&self) -> crate::Result<Option<Self::Read>> {
        let Some(ref current_id) = self.current_id else {
            return Ok(None);
        };
        let snap_dir = self.snapshots_dir().join(current_id.as_str());
        if !snap_dir.exists() {
            return Ok(None);
        }
        let manifest_path = snap_dir.join("manifest.json");
        let raw = match std::fs::read_to_string(&manifest_path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(crate::Error::Io(e)),
        };
        let manifest: SnapshotManifest = serde_json::from_str(&raw).map_err(|e| {
            crate::Error::Index(super::super::super::IndexError::InvalidManifest {
                path: manifest_path,
                source: e,
            })
        })?;
        let lease = SnapshotLease::create_file(&self.dir, current_id.as_str())?;
        Ok(Some(DiskSnapshotReader {
            id: current_id.clone(),
            dir: snap_dir,
            manifest,
            _lease: lease,
        }))
    }

    fn writer(&mut self) -> crate::Result<Self::Writer<'_>> {
        let lock_path = self.dir.join(WRITE_LOCK);
        let mut lock = fslock::LockFile::open(&lock_path)?;
        lock.lock()?;
        Ok(DiskSnapshotWriterSession {
            store: self,
            _lock: lock,
        })
    }
}
