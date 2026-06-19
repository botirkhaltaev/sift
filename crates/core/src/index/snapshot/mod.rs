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

use super::kinds::Index;

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
    indexes: Vec<Index>,
    lease: SnapshotLease,
}

impl Drop for CurrentSnapshot {
    fn drop(&mut self) {
        let _ = &mut self.lease;
    }
}

impl Snapshot {
    #[must_use]
    pub(crate) const fn empty(root: PathBuf) -> Self {
        Self {
            root,
            state: SnapshotState::Empty,
        }
    }

    #[must_use]
    pub(crate) const fn from_indexes(root: PathBuf, indexes: Vec<Index>) -> Self {
        Self {
            root,
            state: SnapshotState::Current(CurrentSnapshot {
                indexes,
                lease: SnapshotLease::InMemory,
            }),
        }
    }

    #[must_use]
    pub(crate) const fn current(root: PathBuf, indexes: Vec<Index>, lease: SnapshotLease) -> Self {
        Self {
            root,
            state: SnapshotState::Current(CurrentSnapshot { indexes, lease }),
        }
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn indexes(&self) -> &[Index] {
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

#[cfg(test)]
mod tests;
