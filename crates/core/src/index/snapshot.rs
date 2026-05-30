use std::path::{Path, PathBuf};

const SNAPSHOTS_DIR: &str = "snapshots";
const LEASES_DIR: &str = "leases";
const CURRENT_FILE: &str = "CURRENT";

// ---------------------------------------------------------------------------
// Write path
// ---------------------------------------------------------------------------

/// An in-progress snapshot transaction backed by a temporary directory.
///
/// The caller writes files into [`dir()`](SnapshotTransaction::dir), then
/// either commits via [`SnapshotStore::commit`] or lets the handle drop (which
/// removes the temporary directory automatically).
pub struct SnapshotTransaction {
    id: String,
    dir: PathBuf,
    committed: bool,
}

impl SnapshotTransaction {
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

impl Drop for SnapshotTransaction {
    fn drop(&mut self) {
        if !self.committed {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

// ---------------------------------------------------------------------------
// Read path
// ---------------------------------------------------------------------------

/// A lease that prevents a snapshot from being garbage-collected.
///
/// Exists publicly so `Indexes::from_single` can pass a no-op variant.
pub trait Lease: std::fmt::Debug + Send + Sync {
    /// Called when the snapshot is dropped — the lease should be released.
    fn release(&self);
}

/// A no-op lease used for in-memory snapshots without a backing store.
#[derive(Debug)]
pub struct NoopLease;

impl Lease for NoopLease {
    fn release(&self) {}
}

/// On-disk lease that prevents a snapshot from being garbage-collected.
#[derive(Debug)]
pub struct FileLease {
    path: PathBuf,
}

impl FileLease {
    pub(crate) fn create(sift_dir: &Path, snapshot_id: &str) -> crate::Result<Box<dyn Lease>> {
        let leases_dir = sift_dir.join(LEASES_DIR);
        std::fs::create_dir_all(&leases_dir)?;
        let pid = std::process::id();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let lease_path = leases_dir.join(format!("{pid}-{now}"));
        std::fs::write(&lease_path, snapshot_id)?;
        Ok(Box::new(Self { path: lease_path }))
    }
}

impl Lease for FileLease {
    fn release(&self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

impl Drop for FileLease {
    fn drop(&mut self) {
        self.release();
    }
}

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
    #[allow(dead_code)]
    id: String,
    indexes: Vec<super::kinds::Index>,
    _lease: Box<dyn Lease>,
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
    /// backing store. Useful for testing and benchmarking where indexes
    /// are kept alive by the caller.
    #[must_use]
    pub fn from_indexes(root: PathBuf, indexes: Vec<super::kinds::Index>) -> Self {
        Self {
            root,
            state: SnapshotState::Current(CurrentSnapshot {
                id: String::new(),
                indexes,
                _lease: Box::new(NoopLease),
            }),
        }
    }

    /// Create a current snapshot with opened indexes, backed by an on-disk
    /// lease that prevents GC from collecting the snapshot.
    pub(crate) fn current_with_lease(
        root: PathBuf,
        id: String,
        indexes: Vec<super::kinds::Index>,
        lease: Box<dyn Lease>,
    ) -> Self {
        Self {
            root,
            state: SnapshotState::Current(CurrentSnapshot {
                id,
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
// Snapshot store
// ---------------------------------------------------------------------------

/// Atomic directory-level snapshot store with a `CURRENT` pointer, writer
/// coordination, reader leases, and generational garbage collection.
///
/// Each snapshot is a directory under `<root>/snapshots/`. Leases in
/// `<root>/leases/` prevent active snapshots from being removed.
pub struct SnapshotStore {
    dir: PathBuf,
    current_id: Option<String>,
}

impl SnapshotStore {
    pub fn leases_dir(sift_dir: &Path) -> PathBuf {
        sift_dir.join(LEASES_DIR)
    }

    /// Read `CURRENT` from disk afresh.
    pub fn read_current_id(sift_dir: &Path) -> crate::Result<Option<String>> {
        let current_path = sift_dir.join(CURRENT_FILE);
        if current_path.exists() {
            Self::read_current(&current_path).map(Some)
        } else {
            Ok(None)
        }
    }

    /// Read active lease files and return the set of snapshot IDs they
    /// reference.
    pub fn active_lease_ids(sift_dir: &Path) -> crate::Result<Vec<String>> {
        let leases_dir = Self::leases_dir(sift_dir);
        let Ok(entries) = std::fs::read_dir(&leases_dir) else {
            return Ok(Vec::new());
        };
        let mut ids = Vec::new();
        for entry in entries {
            let entry = entry?;
            if entry.file_type().is_ok_and(|t| t.is_file())
                && let Ok(raw) = std::fs::read_to_string(entry.path())
            {
                let id = raw.trim().to_string();
                if !id.is_empty() {
                    ids.push(id);
                }
            }
        }
        Ok(ids)
    }

    /// Open (or prepare) a snapshot store rooted at `dir`.
    ///
    /// # Errors
    ///
    /// Returns an error if the `CURRENT` file exists but cannot be read.
    pub fn open(dir: &Path) -> crate::Result<Self> {
        let current_path = dir.join(CURRENT_FILE);
        let current_id = if current_path.exists() {
            Some(Self::read_current(&current_path)?)
        } else {
            None
        };

        Ok(Self {
            dir: dir.to_path_buf(),
            current_id,
        })
    }

    #[must_use]
    pub fn current_id(&self) -> Option<&str> {
        self.current_id.as_deref()
    }

    #[must_use]
    pub fn current_dir(&self) -> Option<PathBuf> {
        self.current_id
            .as_deref()
            .map(|id| self.dir.join(SNAPSHOTS_DIR).join(id))
    }

    /// Begin a new snapshot transaction.
    ///
    /// Creates a temporary directory that the caller populates before
    /// calling [`commit`](Self::commit).
    ///
    /// # Errors
    ///
    /// Returns an error if the snapshots directory cannot be created.
    pub fn begin(&self) -> crate::Result<SnapshotTransaction> {
        let snapshots_dir = self.dir.join(SNAPSHOTS_DIR);
        std::fs::create_dir_all(&snapshots_dir)?;

        let id = Self::generate_id();
        let tmp_dir = snapshots_dir.join(format!("tmp-{id}"));
        std::fs::create_dir_all(&tmp_dir)?;

        Ok(SnapshotTransaction {
            id,
            dir: tmp_dir,
            committed: false,
        })
    }

    /// Atomically publish a snapshot.
    ///
    /// Renames the temporary directory to its final location, updates the
    /// `CURRENT` pointer, and garbage-collects old snapshots.
    ///
    /// # Errors
    ///
    /// Returns an error if the rename or `CURRENT` update fails.
    pub fn commit(&mut self, mut txn: SnapshotTransaction) -> crate::Result<()> {
        let snapshots_dir = self.dir.join(SNAPSHOTS_DIR);
        let final_dir = snapshots_dir.join(&txn.id);
        std::fs::rename(&txn.dir, &final_dir)?;
        txn.committed = true;

        let current_path = self.dir.join(CURRENT_FILE);
        Self::write_atomic(&current_path, &txn.id)?;

        let old_current = self.current_id.replace(txn.id.clone());
        let mut keep: Vec<String> = vec![txn.id.clone()];
        if let Some(ref old_id) = old_current {
            keep.push(old_id.clone());
        }

        // Include any snapshot held by reader leases.
        if let Ok(leased) = Self::active_lease_ids(&self.dir) {
            for id in leased {
                if !keep.contains(&id) {
                    keep.push(id);
                }
            }
        }

        Self::gc(&snapshots_dir, &keep)?;

        Ok(())
    }

    /// Recursively copy all contents from `src` into `dst`.
    ///
    /// # Errors
    ///
    /// Returns an error if any file or directory cannot be read or written.
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

    fn generate_id() -> String {
        let d = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        format!("{:010x}-{:08x}", d.as_secs(), d.subsec_nanos())
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

    fn gc(snapshots_dir: &Path, keep: &[String]) -> crate::Result<()> {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn begin_commit_creates_current() {
        let tmp = TempDir::new().expect("create temp dir");
        let mut store = SnapshotStore::open(tmp.path()).expect("open store");
        assert!(store.current_id().is_none());

        let txn = store.begin().expect("begin snapshot");
        let id = txn.id().to_string();
        std::fs::write(txn.dir().join("data.txt"), "hello").expect("write data");

        store.commit(txn).expect("commit snapshot");
        assert_eq!(store.current_id(), Some(id.as_str()));
        assert!(store.current_dir().expect("has dir").exists());
        assert!(store.current_dir().unwrap().join("data.txt").exists());
    }

    #[test]
    fn drop_without_commit_cleans_tmp() {
        let tmp = TempDir::new().expect("create temp dir");
        let store = SnapshotStore::open(tmp.path()).expect("open store");

        let txn = store.begin().expect("begin snapshot");
        let tmp_dir = txn.dir().to_path_buf();
        assert!(tmp_dir.exists());

        drop(txn);
        assert!(!tmp_dir.exists());
    }

    #[test]
    fn gc_removes_stale_temp_dirs() {
        let tmp = TempDir::new().expect("create temp dir");
        let snapshots_dir = tmp.path().join(SNAPSHOTS_DIR);
        std::fs::create_dir_all(&snapshots_dir).expect("create snapshots dir");
        std::fs::create_dir_all(snapshots_dir.join("tmp-stale")).expect("create stale tmp");
        std::fs::create_dir_all(snapshots_dir.join("0000000000000001")).expect("create snapshot");

        SnapshotStore::gc(&snapshots_dir, &["0000000000000001".to_string()]).expect("gc");

        assert!(!snapshots_dir.join("tmp-stale").exists());
        assert!(snapshots_dir.join("0000000000000001").exists());
    }

    #[test]
    fn commit_gc_keeps_current_and_previous() {
        let tmp = TempDir::new().expect("create temp dir");
        let mut store = SnapshotStore::open(tmp.path()).expect("open store");

        let t1 = store.begin().expect("begin");
        let id1 = t1.id().to_string();
        store.commit(t1).expect("commit");

        let t2 = store.begin().expect("begin");
        let id2 = t2.id().to_string();
        store.commit(t2).expect("commit");

        let snapshots_dir = tmp.path().join(SNAPSHOTS_DIR);
        assert!(snapshots_dir.join(&id2).exists(), "current should exist");
        assert!(snapshots_dir.join(&id1).exists(), "previous should exist");

        let t3 = store.begin().expect("begin");
        let id3 = t3.id().to_string();
        store.commit(t3).expect("commit");

        assert!(snapshots_dir.join(&id3).exists(), "current should exist");
        assert!(snapshots_dir.join(&id2).exists(), "previous should exist");
        assert!(
            !snapshots_dir.join(&id1).exists(),
            "oldest should be cleaned"
        );
    }

    #[test]
    fn leases_prevent_gc() {
        let tmp = TempDir::new().expect("create temp dir");
        let mut store = SnapshotStore::open(tmp.path()).expect("open store");

        let t1 = store.begin().expect("begin");
        let id1 = t1.id().to_string();
        store.commit(t1).expect("commit");

        // Lease on id1 stays alive during the second commit.
        let lease = FileLease::create(tmp.path(), &id1).expect("create lease");

        let t2 = store.begin().expect("begin");
        store.commit(t2).expect("commit");

        let snapshots_dir = tmp.path().join(SNAPSHOTS_DIR);
        assert!(
            snapshots_dir.join(&id1).exists(),
            "leased should survive gc"
        );

        drop(lease);

        let t3 = store.begin().expect("begin");
        let id3 = t3.id().to_string();
        store.commit(t3).expect("commit");

        assert!(
            !snapshots_dir.join(&id1).exists(),
            "unleased should be cleaned"
        );
        assert!(snapshots_dir.join(&id3).exists(), "current should exist");
    }

    #[test]
    fn snapshot_empty_has_no_indexes() {
        let s = Snapshot::empty(PathBuf::from("/tmp"));
        assert!(s.is_empty());
        assert!(s.indexes().is_empty());
        assert_eq!(s.root(), Path::new("/tmp"));
    }
}
