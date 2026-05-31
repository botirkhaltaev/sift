use std::path::{Path, PathBuf};

use super::lease::SnapshotLease;
use crate::index::kinds::Index;

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
    _lease: SnapshotLease,
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
                _lease: SnapshotLease::InMemory,
            }),
        }
    }

    #[must_use]
    pub(crate) const fn current(root: PathBuf, indexes: Vec<Index>, lease: SnapshotLease) -> Self {
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
