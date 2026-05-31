use std::path::{Path, PathBuf};

pub const LEASES_DIR: &str = "leases";

/// A lease that prevents a snapshot from being garbage-collected.
pub enum SnapshotLease {
    File { path: PathBuf },
    InMemory,
}

impl SnapshotLease {
    pub(crate) fn create_file(sift_dir: &Path, snapshot_id: &str) -> crate::Result<Self> {
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
}

impl Drop for SnapshotLease {
    fn drop(&mut self) {
        if let Self::File { path } = self {
            let _ = std::fs::remove_file(path);
        }
    }
}
