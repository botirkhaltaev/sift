use crate::corpus::filter::FilterAdmission;
use crate::index::IndexCoverage;
use crate::index::kinds::IndexQueryResult;

use crate::candidates::query::{CandidateQuery, IndexQuery};
use crate::candidates::selection::{CandidateCoverage, ScanScope, SnapshotFreshness};
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
        let index_query = query.index_query();
        let snapshot_status = Self::snapshot_status(source, scope);
        let index_status = Self::index_status(source, &narrowed);
        let freshness = scope.freshness().unwrap_or(SnapshotFreshness::Current);
        let discovery = Self::discovery(
            scope,
            coverage,
            index_query,
            index_status,
            snapshot_status,
            freshness,
        );
        let query_result = Self::resolve_query_result(narrowed, coverage);
        CandidatePlan {
            discovery,
            order: scope.order(),
            query_result,
        }
    }

    fn resolve_query_result(
        narrowed: IndexQueryResult,
        coverage: CandidateCoverage,
    ) -> IndexQueryResult {
        if coverage == CandidateCoverage::Complete
            && matches!(narrowed, IndexQueryResult::Matched { .. })
        {
            IndexQueryResult::AllIndexed
        } else {
            narrowed
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
        if source.indexes.session().is_none() {
            IndexStatus::Empty
        } else {
            match query_result {
                IndexQueryResult::Unavailable => IndexStatus::NoCandidateIndex,
                IndexQueryResult::AllIndexed => IndexStatus::AllCandidates,
                IndexQueryResult::Matched { .. } => IndexStatus::CanQuery,
            }
        }
    }

    const fn discovery(
        scope: ScanScope,
        coverage: CandidateCoverage,
        index_query: IndexQuery,
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
                    Self::potential_discovery(index_status, index_query, snapshot_status, freshness)
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
        index_query: IndexQuery,
        snapshot_status: SnapshotStatus,
        freshness: SnapshotFreshness,
    ) -> PlannedDiscovery {
        match (index_status, index_query, freshness) {
            (IndexStatus::Empty, _, _)
            | (_, IndexQuery::Disabled, _)
            | (IndexStatus::NoCandidateIndex, _, SnapshotFreshness::Stale) => {
                PlannedDiscovery::Walk
            }
            (
                IndexStatus::NoCandidateIndex | IndexStatus::AllCandidates,
                _,
                SnapshotFreshness::Current,
            ) => PlannedDiscovery::Index {
                admission: Self::admission(snapshot_status),
            },
            (IndexStatus::AllCandidates, _, SnapshotFreshness::Stale) => match snapshot_status {
                SnapshotStatus::Missing | SnapshotStatus::TrustedComplete => {
                    PlannedDiscovery::Index {
                        admission: Self::admission(snapshot_status),
                    }
                }
                SnapshotStatus::FilterMismatch
                | SnapshotStatus::TrustedLazy
                | SnapshotStatus::StaleComplete => PlannedDiscovery::Walk,
            },
            (IndexStatus::CanQuery, _, _) => match snapshot_status {
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
