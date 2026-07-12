use crate::corpus::CandidateOrder;

/// Behavior when the on-disk snapshot is stale or incomplete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexFallback {
    IndexHitsOnly,
    WalkOnStaleSnapshot,
}

impl IndexFallback {
    #[must_use]
    pub const fn walk_on_stale(self) -> bool {
        matches!(self, Self::WalkOnStaleSnapshot)
    }
}

impl CandidateSelection {
    pub(crate) const fn fallback(self) -> IndexFallback {
        match self {
            Self::None | Self::Walk { .. } => IndexFallback::IndexHitsOnly,
            Self::Index { fallback, .. } => fallback,
        }
    }

    pub(crate) fn order(self) -> CandidateOrder {
        match self {
            Self::None => CandidateOrder::default(),
            Self::Index { order, .. } | Self::Walk { order } => order,
        }
    }
}

/// Where corpus candidates come from for a search run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateSelection {
    /// No corpus candidates (streams/stdin only).
    None,
    /// Discover via the index, with stale-snapshot behavior.
    Index {
        fallback: IndexFallback,
        order: CandidateOrder,
    },
    /// Discover by walking the filesystem.
    Walk { order: CandidateOrder },
}
