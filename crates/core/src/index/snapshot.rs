use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

const SNAPSHOTS_DIR: &str = "snapshots";
const LEASES_DIR: &str = "leases";
const CURRENT_FILE: &str = "CURRENT";

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// Unique identifier for a snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SnapshotId(String);

impl SnapshotId {
    pub fn new(id: String) -> Self {
        Self(id)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SnapshotId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Serialized manifest stored in each snapshot listing the index kinds it
/// contains.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotManifest {
    pub id: SnapshotId,
    pub indexes: Vec<String>,
}

/// Binary artifact data returned by a snapshot reader.
///
/// Backed either by a memory-mapped file (zero-copy, disk) or by an
/// in-memory byte buffer.
#[derive(Debug)]
pub enum ArtifactData {
    Memory(Arc<[u8]>),
    Mmap(memmap2::Mmap),
}

impl AsRef<[u8]> for ArtifactData {
    fn as_ref(&self) -> &[u8] {
        match self {
            Self::Memory(bytes) => bytes,
            Self::Mmap(mmap) => mmap.as_ref(),
        }
    }
}

// ---------------------------------------------------------------------------
// Traits
// ---------------------------------------------------------------------------

/// A readable snapshot with access to its id, manifest, and named artifacts.
pub trait SnapshotRead {
    fn id(&self) -> &SnapshotId;
    fn manifest(&self) -> &SnapshotManifest;
    fn artifact(&self, namespace: &str, name: &str) -> crate::Result<ArtifactData>;
}

/// A writable snapshot transaction that accepts named byte artifacts.
pub trait SnapshotWrite {
    fn id(&self) -> &SnapshotId;
    fn put_artifact(&mut self, namespace: &str, name: &str, bytes: Vec<u8>)
        -> crate::Result<()>;
}

/// A scoped writer session that serialises access to the snapshot store.
///
/// Dropping the session releases any exclusive access (e.g. a file lock).
pub trait SnapshotWriterSession {
    type Read: SnapshotRead;
    type Write: SnapshotWrite;

    fn current_id(&self) -> Option<&SnapshotId>;
    fn current(&self) -> crate::Result<Option<Self::Read>>;
    fn begin(&mut self) -> crate::Result<Self::Write>;
    fn publish(
        &mut self,
        write: Self::Write,
        manifest: SnapshotManifest,
    ) -> crate::Result<SnapshotId>;
}

/// Generic atomic snapshot store.
///
/// Provides read access to the current snapshot and scoped write access via
/// [`writer()`](SnapshotStore::writer).
pub trait SnapshotStore {
    type Read: SnapshotRead;
    type Write: SnapshotWrite;
    type Writer<'a>: SnapshotWriterSession<Read = Self::Read, Write = Self::Write>
    where
        Self: 'a;

    fn current_id(&self) -> Option<&SnapshotId>;
    fn current(&self) -> crate::Result<Option<Self::Read>>;
    fn writer(&mut self) -> crate::Result<Self::Writer<'_>>;
}

// ---------------------------------------------------------------------------
// Snapshot lease (disk GC protection)
// ---------------------------------------------------------------------------

/// A lease that prevents a snapshot from being garbage-collected.
pub(crate) enum SnapshotLease {
    File { path: PathBuf },
    InMemory,
}

impl SnapshotLease {
    pub(super) fn create_file(sift_dir: &Path, snapshot_id: &str) -> crate::Result<Self> {
        let leases_dir = sift_dir.join(LEASES_DIR);
        std::fs::create_dir_all(&leases_dir)?;
        let pid = std::process::id();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let leaf = format!("{pid}-{now}");
        let lease_path = leases_dir.join(&leaf);
        let tmp_path = leases_dir.join(format!("{leaf}.tmp"));
        std::fs::write(&tmp_path, snapshot_id)?;
        std::fs::rename(&tmp_path, &lease_path)?;
        Ok(Self::File { path: lease_path })
    }

    pub(super) const fn in_memory() -> Self {
        Self::InMemory
    }
}

impl Drop for SnapshotLease {
    fn drop(&mut self) {
        if let Self::File { path } = self {
            let _ = std::fs::remove_file(path);
        }
    }
}

// ---------------------------------------------------------------------------
// Snapshot (opened indexes)
// ---------------------------------------------------------------------------

/// An immutable opened snapshot and the indexes it contains.
pub struct Snapshot {
    root: PathBuf,
    state: SnapshotState,
}

enum SnapshotState {
    Empty,
    Current(CurrentSnapshot),
}

struct CurrentSnapshot {
    indexes: Vec<super::kinds::Index>,
    _lease: SnapshotLease,
}

impl Snapshot {
    /// Create an empty snapshot (no current index).
    #[must_use]
    pub const fn empty(root: PathBuf) -> Self {
        Self {
            root,
            state: SnapshotState::Empty,
        }
    }

    /// Create a snapshot from already-opened indexes without an on-disk
    /// backing store.
    #[must_use]
    pub const fn from_indexes(root: PathBuf, indexes: Vec<super::kinds::Index>) -> Self {
        Self {
            root,
            state: SnapshotState::Current(CurrentSnapshot {
                indexes,
                _lease: SnapshotLease::InMemory,
            }),
        }
    }

    /// Create a current snapshot with opened indexes, backed by a lease that
    /// prevents GC from collecting the snapshot.
    pub(super) const fn current(
        root: PathBuf,
        indexes: Vec<super::kinds::Index>,
        lease: SnapshotLease,
    ) -> Self {
        Self {
            root,
            state: SnapshotState::Current(CurrentSnapshot {
                indexes,
                _lease: lease,
            }),
        }
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn indexes(&self) -> &[super::kinds::Index] {
        match &self.state {
            SnapshotState::Empty => &[],
            SnapshotState::Current(c) => &c.indexes,
        }
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        matches!(self.state, SnapshotState::Empty)
    }
}

// ---------------------------------------------------------------------------
// On-disk snapshot implementation
// ---------------------------------------------------------------------------

const WRITE_LOCK: &str = "write.lock";

/// On-disk snapshot store backed by a `.sift` directory layout with
/// `snapshots/`, `CURRENT`, `leases/`, and `write.lock`.
pub struct OnDiskSnapshotStore {
    dir: PathBuf,
    current_id: Option<SnapshotId>,
}

impl OnDiskSnapshotStore {
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

    /// Create a lease file for `snapshot_id` in the leases directory.
    pub(crate) fn create_lease(&self, snapshot_id: &SnapshotId) -> crate::Result<SnapshotLease> {
        SnapshotLease::create_file(&self.dir, snapshot_id.as_str())
    }

    pub fn leases_dir(&self) -> PathBuf {
        self.dir.join(LEASES_DIR)
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
        let leases_dir = dir.join(LEASES_DIR);
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

    /// Recursively copy all contents from `src` into `dst`.
    pub(crate) fn copy_dir(src: &Path, dst: &Path) -> crate::Result<()> {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            let ft = entry.file_type()?;
            let dest_path = dst.join(entry.file_name());
            if ft.is_dir() {
                Self::copy_dir(&entry.path(), &dest_path)?;
            } else {
                std::fs::copy(entry.path(), &dest_path)?;
            }
        }
        Ok(())
    }
}

/// A writer session that holds the write lock for an [`OnDiskSnapshotStore`].
pub struct OnDiskWriterSession<'a> {
    store: &'a mut OnDiskSnapshotStore,
    lock: fslock::LockFile,
}

impl Drop for OnDiskWriterSession<'_> {
    fn drop(&mut self) {
        // Lock is released when `lock` is dropped.
    }
}

impl SnapshotWriterSession for OnDiskWriterSession<'_> {
    type Read = OnDiskSnapshotRead;
    type Write = OnDiskSnapshotWrite;

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
            crate::Error::Index(super::IndexError::InvalidManifest {
                path: manifest_path,
                source: e,
            })
        })?;
        let lease = SnapshotLease::create_file(&self.store.dir, current_id.as_str())?;
        Ok(Some(OnDiskSnapshotRead {
            id: current_id.clone(),
            dir: snap_dir,
            manifest,
            _lease: lease,
        }))
    }

    fn begin(&mut self) -> crate::Result<Self::Write> {
        let snapshots_dir = self.store.snapshots_dir();
        std::fs::create_dir_all(&snapshots_dir)?;
        let id_str = OnDiskSnapshotStore::generate_id();
        let id = SnapshotId::new(id_str);
        let tmp_dir = snapshots_dir.join(format!("tmp-{}", id.as_str()));
        std::fs::create_dir_all(&tmp_dir)?;
        Ok(OnDiskSnapshotWrite {
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
        OnDiskSnapshotStore::write_atomic(&current_path, write.id.as_str())?;

        let old_current = self.store.current_id.replace(write.id.clone());
        let mut keep: Vec<String> = vec![write.id.to_string()];
        if let Some(ref old_id) = old_current {
            keep.push(old_id.to_string());
        }

        let leases_dir = self.store.leases_dir();
        OnDiskSnapshotStore::gc(&snapshots_dir, &leases_dir, &keep)?;

        Ok(write.id.clone())
    }
}

/// An in-progress snapshot write backed by a temporary directory on disk.
pub struct OnDiskSnapshotWrite {
    id: SnapshotId,
    dir: PathBuf,
    committed: bool,
}

impl SnapshotWrite for OnDiskSnapshotWrite {
    fn id(&self) -> &SnapshotId {
        &self.id
    }

    fn put_artifact(
        &mut self,
        namespace: &str,
        name: &str,
        bytes: Vec<u8>,
    ) -> crate::Result<()> {
        let dir = self.dir.join(namespace);
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join(name), &bytes)?;
        Ok(())
    }
}

impl OnDiskSnapshotWrite {
    pub(crate) fn dir(&self) -> &Path {
        &self.dir
    }
}

impl Drop for OnDiskSnapshotWrite {
    fn drop(&mut self) {
        if !self.committed {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

/// A readable snapshot backed by on-disk files.
pub struct OnDiskSnapshotRead {
    id: SnapshotId,
    dir: PathBuf,
    manifest: SnapshotManifest,
    _lease: SnapshotLease,
}

impl OnDiskSnapshotRead {
    pub(crate) fn dir(&self) -> &Path {
        &self.dir
    }
}

impl SnapshotRead for OnDiskSnapshotRead {
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
                format!("artifact {namespace}/{name} not found in snapshot {}", self.id),
            )));
        }
        let file = std::fs::File::open(&path)?;
        // SAFETY: memmap2::Mmap::map is unsafe because it dereferences
        // the raw OS mapping pointer. The OS manages bounds and the
        // mapping outlives the closed File handle via refcount.
        #[allow(unsafe_code)]
        let mmap = unsafe { memmap2::Mmap::map(&file) }?;
        Ok(ArtifactData::Mmap(mmap))
    }
}

// ---------------------------------------------------------------------------
// SnapshotStore impl for OnDiskSnapshotStore
// ---------------------------------------------------------------------------

impl SnapshotStore for OnDiskSnapshotStore {
    type Read = OnDiskSnapshotRead;
    type Write = OnDiskSnapshotWrite;
    type Writer<'a> = OnDiskWriterSession<'a>;

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
            crate::Error::Index(super::IndexError::InvalidManifest {
                path: manifest_path,
                source: e,
            })
        })?;
        let lease = SnapshotLease::create_file(&self.dir, current_id.as_str())?;
        Ok(Some(OnDiskSnapshotRead {
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
        Ok(OnDiskWriterSession {
            store: self,
            lock,
        })
    }
}

// ---------------------------------------------------------------------------
// In-memory snapshot implementation
// ---------------------------------------------------------------------------

/// Pure in-memory snapshot store with no filesystem access.
pub struct InMemorySnapshotStore {
    snapshots: BTreeMap<SnapshotId, InMemorySnapshotData>,
    current_id: Option<SnapshotId>,
    next_id: u64,
}

struct InMemorySnapshotData {
    manifest: SnapshotManifest,
    artifacts: BTreeMap<String, BTreeMap<String, Arc<[u8]>>>,
}

impl InMemorySnapshotStore {
    pub fn new() -> Self {
        Self {
            snapshots: BTreeMap::new(),
            current_id: None,
            next_id: 0,
        }
    }
}

impl Default for InMemorySnapshotStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Writer session for [`InMemorySnapshotStore`] (no-op locking).
pub struct InMemoryWriterSession<'a> {
    store: &'a mut InMemorySnapshotStore,
}

impl SnapshotWriterSession for InMemoryWriterSession<'_> {
    type Read = InMemorySnapshotRead;
    type Write = InMemorySnapshotWrite;

    fn current_id(&self) -> Option<&SnapshotId> {
        self.store.current_id.as_ref()
    }

    fn current(&self) -> crate::Result<Option<Self::Read>> {
        let Some(ref id) = self.store.current_id else {
            return Ok(None);
        };
        let data = self.store.snapshots.get(id).ok_or_else(|| {
            crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "current snapshot missing from in-memory store",
            ))
        })?;
        Ok(Some(InMemorySnapshotRead {
            id: id.clone(),
            manifest: data.manifest.clone(),
            artifacts: data.artifacts.clone(),
        }))
    }

    fn begin(&mut self) -> crate::Result<Self::Write> {
        let id_num = self.store.next_id;
        self.store.next_id += 1;
        let id = SnapshotId::new(format!("mem-{id_num:020x}"));
        Ok(InMemorySnapshotWrite {
            id,
            artifacts: BTreeMap::new(),
        })
    }

    fn publish(
        &mut self,
        write: Self::Write,
        manifest: SnapshotManifest,
    ) -> crate::Result<SnapshotId> {
        let data = InMemorySnapshotData {
            manifest,
            artifacts: write.artifacts,
        };
        let id = write.id.clone();
        self.store.snapshots.insert(id.clone(), data);
        self.store.current_id = Some(id.clone());
        Ok(id)
    }
}

/// An in-progress snapshot write backed by a memory buffer.
pub struct InMemorySnapshotWrite {
    id: SnapshotId,
    artifacts: BTreeMap<String, BTreeMap<String, Arc<[u8]>>>,
}

impl SnapshotWrite for InMemorySnapshotWrite {
    fn id(&self) -> &SnapshotId {
        &self.id
    }

    fn put_artifact(
        &mut self,
        namespace: &str,
        name: &str,
        bytes: Vec<u8>,
    ) -> crate::Result<()> {
        self.artifacts
            .entry(namespace.to_string())
            .or_default()
            .insert(name.to_string(), Arc::from(bytes));
        Ok(())
    }
}

/// A readable snapshot backed by in-memory buffers.
#[derive(Clone)]
pub struct InMemorySnapshotRead {
    id: SnapshotId,
    manifest: SnapshotManifest,
    artifacts: BTreeMap<String, BTreeMap<String, Arc<[u8]>>>,
}

impl SnapshotRead for InMemorySnapshotRead {
    fn id(&self) -> &SnapshotId {
        &self.id
    }

    fn manifest(&self) -> &SnapshotManifest {
        &self.manifest
    }

    fn artifact(&self, namespace: &str, name: &str) -> crate::Result<ArtifactData> {
        let ns = self.artifacts.get(namespace).ok_or_else(|| {
            crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("namespace {namespace} not found"),
            ))
        })?;
        let bytes = ns.get(name).ok_or_else(|| {
            crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("artifact {namespace}/{name} not found"),
            ))
        })?;
        Ok(ArtifactData::Memory(bytes.clone()))
    }
}

impl SnapshotStore for InMemorySnapshotStore {
    type Read = InMemorySnapshotRead;
    type Write = InMemorySnapshotWrite;
    type Writer<'a> = InMemoryWriterSession<'a>;

    fn current_id(&self) -> Option<&SnapshotId> {
        self.current_id.as_ref()
    }

    fn current(&self) -> crate::Result<Option<Self::Read>> {
        let Some(ref id) = self.current_id else {
            return Ok(None);
        };
        let data = self.snapshots.get(id).ok_or_else(|| {
            crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "current snapshot missing from in-memory store",
            ))
        })?;
        Ok(Some(InMemorySnapshotRead {
            id: id.clone(),
            manifest: data.manifest.clone(),
            artifacts: data.artifacts.clone(),
        }))
    }

    fn writer(&mut self) -> crate::Result<Self::Writer<'_>> {
        Ok(InMemoryWriterSession { store: self })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // --- On-disk store tests (adapted from previous SnapshotStore tests) ---

    #[test]
    fn disk_begin_commit_creates_current() {
        let tmp = TempDir::new().expect("create temp dir");
        let mut store = OnDiskSnapshotStore::open(tmp.path()).expect("open store");
        assert!(store.current_id.is_none());

        {
            let mut session = store.writer().expect("writer");
            let mut txn = session.begin().expect("begin");
            txn.put_artifact("test", "data.txt", b"hello".to_vec())
                .expect("put artifact");

            let manifest = SnapshotManifest {
                id: txn.id().clone(),
                indexes: vec![],
            };
            session.publish(txn, manifest).expect("publish");
        }

        assert!(store.current_id.is_some());
        let snap_dir = store.snapshots_dir().join(store.current_id.as_ref().unwrap().as_str());
        assert!(snap_dir.exists());
        assert!(snap_dir.join("test").join("data.txt").exists());

        // Re-open to verify persistence.
        let store2 = OnDiskSnapshotStore::open(tmp.path()).expect("reopen");
        assert!(store2.current_id.is_some());
        assert_eq!(store2.current_id, store.current_id);
    }

    #[test]
    fn disk_drop_without_commit_cleans_tmp() {
        let tmp = TempDir::new().expect("create temp dir");
        let mut store = OnDiskSnapshotStore::open(tmp.path()).expect("open store");

        let mut session = store.writer().expect("writer");
        let txn = session.begin().expect("begin");
        let tmp_dir = txn.dir.clone();
        assert!(tmp_dir.exists());
        drop(txn);
        assert!(!tmp_dir.exists());
    }

    #[test]
    fn in_memory_begin_commit_creates_current() {
        let mut store = InMemorySnapshotStore::new();
        assert!(store.current_id().is_none());

        let mut session = store.writer().expect("writer");
        let mut txn = session.begin().expect("begin");
        txn.put_artifact("test", "data.txt", b"hello".to_vec())
            .expect("put artifact");

        let manifest = SnapshotManifest {
            id: txn.id().clone(),
            indexes: vec![],
        };
        let id = session.publish(txn, manifest).expect("publish");
        assert_eq!(store.current_id().unwrap(), &id);
    }

    #[test]
    fn in_memory_current_returns_snapshot() {
        let mut store = InMemorySnapshotStore::new();
        assert!(store.current().unwrap().is_none());

        let mut session = store.writer().expect("writer");
        let mut txn = session.begin().expect("begin");
        txn.put_artifact("test", "data.txt", b"content".to_vec())
            .expect("put");

        let manifest = SnapshotManifest {
            id: txn.id().clone(),
            indexes: vec!["trigram".to_string()],
        };
        let id = session.publish(txn, manifest).expect("publish");

        let current = store.current().unwrap().expect("has current");
        assert_eq!(current.id(), &id);
        assert_eq!(current.manifest().indexes, &["trigram"]);
        let artifact = current.artifact("test", "data.txt").unwrap();
        assert_eq!(artifact.as_ref(), b"content");
    }

    #[test]
    fn in_memory_writer_sees_new_current() {
        let mut store = InMemorySnapshotStore::new();

        // Publish first snapshot.
        {
            let mut session = store.writer().expect("writer");
            let txn = session.begin().expect("begin");
            let manifest = SnapshotManifest {
                id: txn.id().clone(),
                indexes: vec![],
            };
            let id1 = session.publish(txn, manifest).expect("publish");
            assert_eq!(session.current_id().unwrap(), &id1);
            assert!(session.current().unwrap().is_some());
        }

        // Publish second snapshot — writer session sees the new current.
        {
            let mut session = store.writer().expect("writer");
            let prev = session.current().unwrap().expect("prev");
            let txn = session.begin().expect("begin");
            let manifest = SnapshotManifest {
                id: txn.id().clone(),
                indexes: vec![],
            };
            let id2 = session.publish(txn, manifest).expect("publish");
            assert_eq!(session.current_id().unwrap(), &id2);
            assert_ne!(&id2, prev.id());
        }
    }
}
