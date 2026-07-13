use crate::corpus::filter::FilterAdmission;
use crate::index::IndexCoverage;
use crate::index::kinds::IndexQueryResult;

use crate::candidates::query::{CandidateQuery, PrefilterNarrowing};
use crate::candidates::scope::{CandidateCoverage, IndexNarrowing, ScanScope, SnapshotFreshness};
use crate::candidates::source::CandidateSource;

use super::plan::{CandidatePlan, PlannedDiscovery};

pub(crate) struct CandidatePlanner;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IndexStatus {
    Empty,
    NoCandidateIndex,
    AllCandidates,
    CanQuery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SnapshotStatus {
    Missing,
    FilterMismatch,
    TrustedComplete,
    TrustedLazy,
    StaleComplete,
}

impl CandidatePlanner {
    /// Pure decision: no filesystem or index reads beyond cheap metadata.
    pub(crate) fn plan(
        source: &CandidateSource<'_>,
        query: &CandidateQuery<'_>,
        coverage: CandidateCoverage,
    ) -> CandidatePlan {
        let scope = source.scope;
        let narrowed = source.indexes.query(query);
        let narrowing = Self::narrowing_allowed(source, query);
        let snapshot_status = Self::snapshot_status(source, scope);
        let index_status = Self::index_status(source, &narrowed);
        let freshness = scope.freshness().unwrap_or(SnapshotFreshness::Current);
        let discovery = Self::discovery(
            scope,
            coverage,
            narrowing,
            index_status,
            snapshot_status,
            freshness,
        );
        let query_result = Self::plan_query_result(discovery, coverage, narrowed);
        CandidatePlan {
            discovery,
            order: scope.order(),
            query_result,
            indexed_corpus: source.indexes.indexed_corpus(),
        }
    }

    const fn narrowing_allowed(source: &CandidateSource<'_>, query: &CandidateQuery<'_>) -> bool {
        matches!(source.index_narrowing, IndexNarrowing::Allowed)
            && matches!(query.prefilter_narrowing(), PrefilterNarrowing::Allowed)
    }

    fn plan_query_result(
        discovery: PlannedDiscovery,
        coverage: CandidateCoverage,
        narrowed: IndexQueryResult,
    ) -> IndexQueryResult {
        match (coverage, discovery) {
            (
                CandidateCoverage::Complete,
                PlannedDiscovery::Index { .. } | PlannedDiscovery::Merge { .. },
            ) => IndexQueryResult::AllIndexed,
            _ => narrowed,
        }
    }

    fn snapshot_status(source: &CandidateSource<'_>, scope: ScanScope) -> SnapshotStatus {
        if !matches!(scope, ScanScope::Index { .. }) {
            return SnapshotStatus::Missing;
        }
        let freshness = scope.freshness().unwrap_or(SnapshotFreshness::Current);
        let Some(meta) = source.store_meta else {
            return SnapshotStatus::Missing;
        };
        if !meta.covers_candidate_filter(source.filter) {
            return SnapshotStatus::FilterMismatch;
        }
        match meta.coverage {
            IndexCoverage::Complete if freshness == SnapshotFreshness::Stale => {
                SnapshotStatus::StaleComplete
            }
            IndexCoverage::Complete => SnapshotStatus::TrustedComplete,
            IndexCoverage::Lazy => SnapshotStatus::TrustedLazy,
        }
    }

    fn index_status(source: &CandidateSource<'_>, query_result: &IndexQueryResult) -> IndexStatus {
        if source.indexes.usable() {
            match query_result {
                IndexQueryResult::Unavailable => IndexStatus::NoCandidateIndex,
                IndexQueryResult::AllIndexed => IndexStatus::AllCandidates,
                IndexQueryResult::Matched { .. } => IndexStatus::CanQuery,
            }
        } else {
            IndexStatus::Empty
        }
    }

    const fn discovery(
        scope: ScanScope,
        coverage: CandidateCoverage,
        narrowing: bool,
        index_status: IndexStatus,
        snapshot_status: SnapshotStatus,
        freshness: SnapshotFreshness,
    ) -> PlannedDiscovery {
        match scope {
            ScanScope::StreamsOnly => PlannedDiscovery::Empty,
            ScanScope::Walk { .. } => PlannedDiscovery::Walk,
            ScanScope::Index { .. } => match coverage {
                CandidateCoverage::Complete => {
                    Self::complete_discovery(index_status, snapshot_status)
                }
                CandidateCoverage::PotentialMatches => {
                    Self::potential_discovery(index_status, narrowing, snapshot_status, freshness)
                }
            },
        }
    }

    const fn complete_discovery(
        index_status: IndexStatus,
        snapshot_status: SnapshotStatus,
    ) -> PlannedDiscovery {
        match (index_status, snapshot_status) {
            (IndexStatus::Empty, _)
            | (
                _,
                SnapshotStatus::FilterMismatch
                | SnapshotStatus::TrustedLazy
                | SnapshotStatus::StaleComplete,
            ) => PlannedDiscovery::Walk,
            (_, _) => PlannedDiscovery::Index {
                admission: Self::admission(snapshot_status),
            },
        }
    }

    const fn potential_discovery(
        index_status: IndexStatus,
        narrowing: bool,
        snapshot_status: SnapshotStatus,
        freshness: SnapshotFreshness,
    ) -> PlannedDiscovery {
        match (index_status, narrowing, freshness) {
            (IndexStatus::Empty, _, _)
            | (_, false, _)
            | (IndexStatus::NoCandidateIndex, _, SnapshotFreshness::Stale) => {
                PlannedDiscovery::Walk
            }
            (
                IndexStatus::NoCandidateIndex | IndexStatus::AllCandidates,
                true,
                SnapshotFreshness::Current,
            ) => PlannedDiscovery::Index {
                admission: Self::admission(snapshot_status),
            },
            (IndexStatus::AllCandidates, true, SnapshotFreshness::Stale) => match snapshot_status {
                SnapshotStatus::Missing | SnapshotStatus::TrustedComplete => {
                    PlannedDiscovery::Index {
                        admission: Self::admission(snapshot_status),
                    }
                }
                SnapshotStatus::FilterMismatch
                | SnapshotStatus::TrustedLazy
                | SnapshotStatus::StaleComplete => PlannedDiscovery::Walk,
            },
            (IndexStatus::CanQuery, true, _) => match snapshot_status {
                SnapshotStatus::TrustedLazy => PlannedDiscovery::Merge {
                    admission: Self::admission(snapshot_status),
                },
                _ => PlannedDiscovery::Index {
                    admission: Self::admission(snapshot_status),
                },
            },
        }
    }

    const fn admission(snapshot_status: SnapshotStatus) -> FilterAdmission {
        match snapshot_status {
            SnapshotStatus::TrustedComplete
            | SnapshotStatus::TrustedLazy
            | SnapshotStatus::StaleComplete => FilterAdmission::Indexed,
            SnapshotStatus::Missing | SnapshotStatus::FilterMismatch => FilterAdmission::Full,
        }
    }
}
