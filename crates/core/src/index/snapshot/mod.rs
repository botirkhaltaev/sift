pub mod artifact;
pub mod identity;
pub mod lease;
pub mod manifest;
pub mod store;

pub use artifact::ArtifactData;
pub use identity::SnapshotId;
pub use lease::SnapshotLease;
pub use manifest::SnapshotManifest;
pub use store::disk::DiskSnapshotStore;
pub use store::{SnapshotRead, SnapshotStore, SnapshotWrite, SnapshotWriterSession};

use std::path::{Path, PathBuf};

use super::error::IndexError;
use super::kinds::Index;
use super::meta::StoreMeta;
use super::{IndexConfig, IndexSource};

/// An immutable opened snapshot and the indexes it contains.
pub struct Snapshot {
    id: Option<SnapshotId>,
    root: PathBuf,
    state: SnapshotState,
}

enum SnapshotState {
    Empty,
    Current(CurrentSnapshot),
}

struct CurrentSnapshot {
    indexes: Vec<Index>,
    _lease: SnapshotLease,
}

impl Snapshot {
    #[must_use]
    pub(crate) const fn empty(root: PathBuf) -> Self {
        Self {
            id: None,
            root,
            state: SnapshotState::Empty,
        }
    }

    #[must_use]
    pub(crate) const fn committed(
        id: SnapshotId,
        root: PathBuf,
        indexes: Vec<Index>,
        lease: SnapshotLease,
    ) -> Self {
        Self {
            id: Some(id),
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
    pub const fn id(&self) -> Option<&SnapshotId> {
        self.id.as_ref()
    }

    #[must_use]
    pub(crate) fn indexes(&self) -> &[Index] {
        match &self.state {
            SnapshotState::Empty => &[],
            SnapshotState::Current(c) => &c.indexes,
        }
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        matches!(self.state, SnapshotState::Empty)
    }

    /// Open the current committed snapshot for search.
    ///
    /// # Errors
    ///
    /// Returns an error if the manifest is malformed, an index kind is unknown,
    /// or the snapshot could not be opened after retry.
    pub fn open_current(sift_dir: &Path) -> crate::Result<Self> {
        for attempt in 0..2 {
            let Some(current_id) = DiskSnapshotStore::read_current_id(sift_dir)? else {
                return Ok(Self::empty(PathBuf::new()));
            };

            let meta = StoreMeta::read(sift_dir).map_err(|_| {
                crate::Error::Index(IndexError::Io {
                    path: StoreMeta::path(sift_dir),
                    source: std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "store metadata not found",
                    ),
                })
            })?;
            let root = meta.corpus.root.clone();
            let corpus_kind = meta.corpus.kind;

            let snap_dir = sift_dir.join("snapshots").join(&current_id);

            let lease = SnapshotLease::create_file(sift_dir, &current_id)?;

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

            let manifest: SnapshotManifest = serde_json::from_str(&manifest_raw).map_err(|e| {
                crate::Error::Index(IndexError::InvalidManifest {
                    path: manifest_path.clone(),
                    source: e,
                })
            })?;

            let mut indexes = Vec::new();
            for name in &manifest.indexes {
                let config: IndexConfig = name.parse().map_err(|_| {
                    crate::Error::Index(IndexError::UnknownIndexConfig(name.clone()))
                })?;
                let index_dir = snap_dir.join(name);
                indexes.push(config.open(
                    IndexSource::Directory(&index_dir),
                    &root,
                    corpus_kind,
                )?);
            }

            return Ok(Self::committed(manifest.id, root, indexes, lease));
        }

        Err(crate::Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "snapshot disappeared during open",
        )))
    }
}

#[cfg(test)]
mod tests;
