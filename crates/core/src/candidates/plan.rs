use crate::corpus::filter::FilterAdmission;
use crate::corpus::order::CandidateOrder;
use crate::index::IndexCoverage;
use crate::index::kinds::NarrowingResult;

use super::narrowing::{CandidateQuery, IndexNarrowing};
use super::selection::{CandidateCoverage, CandidateSelection, IndexFallback};
use super::source::CandidateSource;

/// The execution plan for candidate resolution.
#[must_use]
pub(crate) struct CandidatePlan {
    pub discovery: PlannedDiscovery,
    pub order: CandidateOrder,
    pub narrowing: NarrowingResult,
}

/// How candidate discovery will run at resolve time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlannedDiscovery {
    Empty,
    Walk,
    Index {
        admission: FilterAdmission,
    },
    /// Index hits merged with a walk of unindexed paths (lazy snapshots).
    Merge {
        admission: FilterAdmission,
    },
}

pub(crate) struct CandidatePlanner;

impl CandidatePlanner {
    /// Pure decision: no filesystem or index reads beyond cheap metadata.
    pub(crate) fn plan(
        source: &CandidateSource<'_>,
        query: &CandidateQuery<'_>,
        selection: CandidateSelection,
        coverage: CandidateCoverage,
    ) -> CandidatePlan {
        let narrowing = source.indexes.narrow(query);
        let index_narrowing = query.index_narrowing();
        let fallback = selection.fallback();
        let snapshot_status = snapshot_status(source, selection);
        let index_status = index_status(source, &narrowing);
        let discovery = planned_discovery(
            selection,
            coverage,
            index_narrowing,
            index_status,
            snapshot_status,
            fallback,
        );
        CandidatePlan {
            discovery,
            order: selection.order(),
            narrowing,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IndexStatus {
    Empty,
    NoCandidateIndex,
    AllCandidates,
    CanNarrow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SnapshotStatus {
    Missing,
    FilterMismatch,
    TrustedComplete,
    TrustedLazy,
    StaleComplete,
}

fn snapshot_status(source: &CandidateSource<'_>, selection: CandidateSelection) -> SnapshotStatus {
    if matches!(selection, CandidateSelection::None) {
        return SnapshotStatus::Missing;
    }
    let Some(meta) = source.store_meta else {
        return SnapshotStatus::Missing;
    };
    if !meta.covers_candidate_filter(source.filter) {
        return SnapshotStatus::FilterMismatch;
    }
    let fallback = selection.fallback();
    match meta.coverage {
        IndexCoverage::Complete if fallback.walk_on_stale() => SnapshotStatus::StaleComplete,
        IndexCoverage::Complete => SnapshotStatus::TrustedComplete,
        IndexCoverage::Lazy => SnapshotStatus::TrustedLazy,
    }
}

fn index_status(source: &CandidateSource<'_>, narrowing: &NarrowingResult) -> IndexStatus {
    if source.indexes.availability().is_none() {
        IndexStatus::Empty
    } else {
        match narrowing {
            NarrowingResult::Unavailable => IndexStatus::NoCandidateIndex,
            NarrowingResult::AllIndexed => IndexStatus::AllCandidates,
            NarrowingResult::Narrowed { .. } => IndexStatus::CanNarrow,
        }
    }
}

const fn planned_discovery(
    selection: CandidateSelection,
    coverage: CandidateCoverage,
    index_narrowing: IndexNarrowing,
    index_status: IndexStatus,
    snapshot_status: SnapshotStatus,
    fallback: IndexFallback,
) -> PlannedDiscovery {
    match selection {
        CandidateSelection::None => PlannedDiscovery::Empty,
        CandidateSelection::Walk { .. } => PlannedDiscovery::Walk,
        CandidateSelection::Index { .. } => match coverage {
            CandidateCoverage::Complete => plan_complete(index_status, snapshot_status, fallback),
            CandidateCoverage::PotentialMatches => {
                plan_potential(index_status, index_narrowing, snapshot_status, fallback)
            }
        },
    }
}

const fn plan_complete(
    index_status: IndexStatus,
    snapshot_status: SnapshotStatus,
    fallback: IndexFallback,
) -> PlannedDiscovery {
    match (index_status, snapshot_status, fallback) {
        (IndexStatus::Empty, _, _) => PlannedDiscovery::Walk,
        (_, SnapshotStatus::FilterMismatch | SnapshotStatus::TrustedLazy, _)
        | (_, SnapshotStatus::StaleComplete, IndexFallback::WalkOnStaleSnapshot) => {
            PlannedDiscovery::Walk
        }
        (_, _, _) => PlannedDiscovery::Index {
            admission: index_admission(snapshot_status),
        },
    }
}

const fn plan_potential(
    index_status: IndexStatus,
    index_narrowing: IndexNarrowing,
    snapshot_status: SnapshotStatus,
    fallback: IndexFallback,
) -> PlannedDiscovery {
    match (index_status, index_narrowing, fallback) {
        (IndexStatus::Empty, _, _)
        | (_, IndexNarrowing::Disabled, _)
        | (IndexStatus::NoCandidateIndex, _, IndexFallback::WalkOnStaleSnapshot) => {
            PlannedDiscovery::Walk
        }
        (
            IndexStatus::NoCandidateIndex | IndexStatus::AllCandidates,
            _,
            IndexFallback::IndexHitsOnly,
        ) => PlannedDiscovery::Index {
            admission: index_admission(snapshot_status),
        },
        (IndexStatus::AllCandidates, _, IndexFallback::WalkOnStaleSnapshot) => {
            match snapshot_status {
                SnapshotStatus::Missing | SnapshotStatus::TrustedComplete => {
                    PlannedDiscovery::Index {
                        admission: index_admission(snapshot_status),
                    }
                }
                SnapshotStatus::FilterMismatch
                | SnapshotStatus::TrustedLazy
                | SnapshotStatus::StaleComplete => PlannedDiscovery::Walk,
            }
        }
        (IndexStatus::CanNarrow, _, _) => match snapshot_status {
            SnapshotStatus::TrustedLazy => PlannedDiscovery::Merge {
                admission: index_admission(snapshot_status),
            },
            _ => PlannedDiscovery::Index {
                admission: index_admission(snapshot_status),
            },
        },
    }
}

const fn index_admission(snapshot_status: SnapshotStatus) -> FilterAdmission {
    match snapshot_status {
        SnapshotStatus::TrustedComplete
        | SnapshotStatus::TrustedLazy
        | SnapshotStatus::StaleComplete => FilterAdmission::Indexed,
        SnapshotStatus::Missing | SnapshotStatus::FilterMismatch => FilterAdmission::Full,
    }
}
