use std::path::{Path, PathBuf};

const SNAPSHOTS_DIR: &str = "snapshots";
const CURRENT_FILE: &str = "CURRENT";

/// An in-progress snapshot backed by a temporary directory.
///
/// The caller writes files into [`dir()`](Snapshot::dir), then either commits
/// via [`SnapshotStore::commit`] or lets the handle drop (which removes the
/// temporary directory automatically).
pub struct Snapshot {
    id: String,
    dir: PathBuf,
    committed: bool,
}

impl Snapshot {
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

impl Drop for Snapshot {
    fn drop(&mut self) {
        if !self.committed {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

/// Atomic directory-level snapshot store with a `CURRENT` pointer and
/// generational garbage collection.
///
/// Each snapshot is a directory under `<root>/snapshots/`. At most two
/// snapshots are kept on disk (the current and the previous) to allow
/// concurrent readers to finish before their snapshot is removed.
pub struct SnapshotStore {
    dir: PathBuf,
    current_id: Option<String>,
}

impl SnapshotStore {
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
    pub fn begin(&self) -> crate::Result<Snapshot> {
        let snapshots_dir = self.dir.join(SNAPSHOTS_DIR);
        std::fs::create_dir_all(&snapshots_dir)?;

        let id = Self::generate_id();
        let tmp_dir = snapshots_dir.join(format!("tmp-{id}"));
        std::fs::create_dir_all(&tmp_dir)?;

        Ok(Snapshot {
            id,
            dir: tmp_dir,
            committed: false,
        })
    }

    /// Atomically publish a snapshot.
    ///
    /// Renames the temporary directory to its final location, updates the
    /// `CURRENT` pointer, and garbage-collects old snapshots (keeping the
    /// current and previous).
    ///
    /// # Errors
    ///
    /// Returns an error if the rename or `CURRENT` update fails.
    pub fn commit(&mut self, mut snapshot: Snapshot) -> crate::Result<()> {
        let snapshots_dir = self.dir.join(SNAPSHOTS_DIR);
        let final_dir = snapshots_dir.join(&snapshot.id);
        std::fs::rename(&snapshot.dir, &final_dir)?;
        snapshot.committed = true;

        let current_path = self.dir.join(CURRENT_FILE);
        Self::write_atomic(&current_path, &snapshot.id)?;

        let old_current = self.current_id.replace(snapshot.id.clone());
        let mut keep: Vec<&str> = vec![&snapshot.id];
        if let Some(ref old_id) = old_current {
            keep.push(old_id.as_str());
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

    fn gc(snapshots_dir: &Path, keep: &[&str]) -> crate::Result<()> {
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

        let snapshot = store.begin().expect("begin snapshot");
        let id = snapshot.id().to_string();
        std::fs::write(snapshot.dir().join("data.txt"), "hello").expect("write data");

        store.commit(snapshot).expect("commit snapshot");
        assert_eq!(store.current_id(), Some(id.as_str()));
        assert!(store.current_dir().expect("has dir").exists());
        assert!(store.current_dir().unwrap().join("data.txt").exists());
    }

    #[test]
    fn drop_without_commit_cleans_tmp() {
        let tmp = TempDir::new().expect("create temp dir");
        let store = SnapshotStore::open(tmp.path()).expect("open store");

        let snapshot = store.begin().expect("begin snapshot");
        let tmp_dir = snapshot.dir().to_path_buf();
        assert!(tmp_dir.exists());

        drop(snapshot);
        assert!(!tmp_dir.exists());
    }

    #[test]
    fn gc_removes_stale_temp_dirs() {
        let tmp = TempDir::new().expect("create temp dir");
        let snapshots_dir = tmp.path().join(SNAPSHOTS_DIR);
        std::fs::create_dir_all(&snapshots_dir).expect("create snapshots dir");
        std::fs::create_dir_all(snapshots_dir.join("tmp-stale")).expect("create stale tmp");
        std::fs::create_dir_all(snapshots_dir.join("0000000000000001")).expect("create snapshot");

        SnapshotStore::gc(&snapshots_dir, &["0000000000000001"]).expect("gc");

        assert!(!snapshots_dir.join("tmp-stale").exists());
        assert!(snapshots_dir.join("0000000000000001").exists());
    }

    #[test]
    fn commit_gc_keeps_current_and_previous() {
        let tmp = TempDir::new().expect("create temp dir");
        let mut store = SnapshotStore::open(tmp.path()).expect("open store");

        let s1 = store.begin().expect("begin");
        let id1 = s1.id().to_string();
        store.commit(s1).expect("commit");

        let s2 = store.begin().expect("begin");
        let id2 = s2.id().to_string();
        store.commit(s2).expect("commit");

        let snapshots_dir = tmp.path().join(SNAPSHOTS_DIR);
        assert!(snapshots_dir.join(&id2).exists(), "current should exist");
        assert!(snapshots_dir.join(&id1).exists(), "previous should exist");

        let s3 = store.begin().expect("begin");
        let id3 = s3.id().to_string();
        store.commit(s3).expect("commit");

        assert!(snapshots_dir.join(&id3).exists(), "current should exist");
        assert!(snapshots_dir.join(&id2).exists(), "previous should exist");
        assert!(
            !snapshots_dir.join(&id1).exists(),
            "oldest should be cleaned"
        );
    }
}
