use crate::candidates::{
    CandidateCoverage, CandidateQuery, CandidateSelection, CandidateSource, IndexFallback,
};
use crate::corpus::filter::FilterAdmission;
use crate::corpus::order::CandidateOrder;
use crate::index::IndexCoverage;
use crate::index::kinds::NarrowingResult;

use super::plan::{CandidatePlan, PlannedDiscovery};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IndexNarrowing {
    Enabled,
    Disabled,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiscoveryStrategy {
    Empty,
    Walk,
    AllIndexed,
    Narrowed,
    Merge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PlanInput {
    selection: CandidateSelection,
    coverage: CandidateCoverage,
    index_narrowing: IndexNarrowing,
    index_status: IndexStatus,
    snapshot_status: SnapshotStatus,
    fallback: IndexFallback,
}

pub struct CandidatePlanner;

impl CandidatePlanner {
    /// Pure decision: no filesystem or index reads beyond cheap metadata.
    pub fn plan<'src>(
        source: &'src CandidateSource<'src>,
        query: &'src CandidateQuery<'src>,
        selection: CandidateSelection,
        coverage: CandidateCoverage,
    ) -> CandidatePlan<'src> {
        let index_narrowing = query.index_narrowing();
        let fallback = selection.fallback();
        let snapshot_status = snapshot_status(source, selection);
        let narrowing = source.indexes.narrow(query);
        let strategy = plan_strategy(PlanInput {
            selection,
            coverage,
            index_narrowing,
            index_status: index_status(source, &narrowing),
            snapshot_status,
            fallback,
        });
        let discovery = strategy.planned_discovery(snapshot_status);
        let order = selection.order();
        CandidatePlan::new(discovery, order, source, query)
    }
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

const fn plan_strategy(input: PlanInput) -> DiscoveryStrategy {
    match input.selection {
        CandidateSelection::None => DiscoveryStrategy::Empty,
        CandidateSelection::Walk { .. } => DiscoveryStrategy::Walk,
        CandidateSelection::Index { .. } => match input.coverage {
            CandidateCoverage::Complete => plan_complete(input),
            CandidateCoverage::PotentialMatches => plan_potential(input),
        },
    }
}

const fn plan_complete(input: PlanInput) -> DiscoveryStrategy {
    match (input.index_status, input.snapshot_status, input.fallback) {
        (IndexStatus::Empty, _, _) => DiscoveryStrategy::Walk,
        (_, SnapshotStatus::FilterMismatch | SnapshotStatus::TrustedLazy, _)
        | (_, SnapshotStatus::StaleComplete, IndexFallback::WalkOnStaleSnapshot) => {
            DiscoveryStrategy::Walk
        }
        (_, _, _) => DiscoveryStrategy::AllIndexed,
    }
}

const fn plan_potential(input: PlanInput) -> DiscoveryStrategy {
    match (input.index_status, input.index_narrowing, input.fallback) {
        (IndexStatus::Empty, _, _)
        | (_, IndexNarrowing::Disabled, _)
        | (IndexStatus::NoCandidateIndex, _, IndexFallback::WalkOnStaleSnapshot) => {
            DiscoveryStrategy::Walk
        }
        (
            IndexStatus::NoCandidateIndex | IndexStatus::AllCandidates,
            _,
            IndexFallback::IndexHitsOnly,
        ) => DiscoveryStrategy::AllIndexed,
        (IndexStatus::AllCandidates, _, IndexFallback::WalkOnStaleSnapshot) => {
            match input.snapshot_status {
                SnapshotStatus::Missing | SnapshotStatus::TrustedComplete => {
                    DiscoveryStrategy::AllIndexed
                }
                SnapshotStatus::FilterMismatch
                | SnapshotStatus::TrustedLazy
                | SnapshotStatus::StaleComplete => DiscoveryStrategy::Walk,
            }
        }
        (IndexStatus::CanNarrow, _, _) => match input.snapshot_status {
            SnapshotStatus::TrustedLazy => DiscoveryStrategy::Merge,
            _ => DiscoveryStrategy::Narrowed,
        },
    }
}

impl DiscoveryStrategy {
    const fn planned_discovery(self, snapshot_status: SnapshotStatus) -> PlannedDiscovery {
        match self {
            Self::Empty => PlannedDiscovery::Empty,
            Self::Walk => PlannedDiscovery::Walk,
            Self::AllIndexed | Self::Narrowed => PlannedDiscovery::Index {
                admission: self.filter_admission(snapshot_status),
            },
            Self::Merge => PlannedDiscovery::Merge,
        }
    }

    const fn filter_admission(self, snapshot_status: SnapshotStatus) -> FilterAdmission {
        match (self, snapshot_status) {
            (
                Self::Narrowed | Self::AllIndexed,
                SnapshotStatus::TrustedComplete
                | SnapshotStatus::TrustedLazy
                | SnapshotStatus::StaleComplete,
            ) => FilterAdmission::Indexed,
            _ => FilterAdmission::Full,
        }
    }
}

impl CandidateSelection {
    const fn fallback(self) -> IndexFallback {
        match self {
            Self::None | Self::Walk { .. } => IndexFallback::IndexHitsOnly,
            Self::Index { fallback, .. } => fallback,
        }
    }

    fn order(self) -> CandidateOrder {
        match self {
            Self::None => CandidateOrder::default(),
            Self::Index { order, .. } | Self::Walk { order } => order,
        }
    }
}
