use std::collections::HashSet;
use std::path::PathBuf;

use rayon::prelude::*;

use crate::IndexCoverage;
use crate::candidates::indexed::{CandidateMaterialization, IndexedCandidates, ResolvedCandidates};
use crate::candidates::{
    CandidateRequest, CandidateScope, CandidateSource, CandidateSpec, IndexFallback,
};
use crate::corpus::Candidate;
use crate::corpus::filter::FilterAdmission;
use crate::corpus::walk::FileWalk;
use crate::index::{CandidatePlan, MaterializeRequest};

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
enum CandidateStrategy {
    None,
    UseIndex,
    Walk,
    AllIndexed,
    MergeIndexAndWalk,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PlanInput {
    scope: CandidateScope,
    index_narrowing: IndexNarrowing,
    index_status: IndexStatus,
    snapshot_status: SnapshotStatus,
    fallback: IndexFallback,
}

pub struct CandidatePlanner<'a> {
    source: &'a CandidateSource<'a>,
    spec: CandidateSpec<'a>,
    request: CandidateRequest,
}

struct CandidateSet {
    candidates: Vec<Candidate>,
}

impl<'a> CandidatePlanner<'a> {
    #[must_use]
    pub const fn new(
        source: &'a CandidateSource<'a>,
        spec: CandidateSpec<'a>,
        request: CandidateRequest,
    ) -> Self {
        Self {
            source,
            spec,
            request,
        }
    }

    /// Plan and resolve candidates for a query.
    ///
    /// # Errors
    ///
    /// Returns an error if filesystem walking or ordering fails.
    pub fn resolve(
        self,
        materialization: CandidateMaterialization,
    ) -> crate::Result<ResolvedCandidates<'a>> {
        let index_narrowing = self.spec.index_narrowing();
        let resolved = self.request.resolve(index_narrowing);
        let index_plan = self.source.indexes.plan(&self.spec);
        let snapshot_status = self.snapshot_status();
        let strategy = plan(PlanInput {
            scope: resolved.scope,
            index_narrowing,
            index_status: self.index_status(&index_plan),
            snapshot_status,
            fallback: resolved.fallback,
        });
        self.execute(
            strategy,
            index_plan,
            resolved.order,
            snapshot_status,
            materialization,
        )
    }

    fn execute(
        self,
        strategy: CandidateStrategy,
        index_plan: CandidatePlan,
        order: crate::corpus::CandidateOrder,
        snapshot_status: SnapshotStatus,
        materialization: CandidateMaterialization,
    ) -> crate::Result<ResolvedCandidates<'a>> {
        let admission = strategy.filter_admission(snapshot_status);
        let matching = MaterializeRequest::Matching {
            filter: self.source.filter,
            admission,
        };
        let defer = matches!(materialization, CandidateMaterialization::Deferred)
            && !order.is_sorted()
            && self.source.indexes.is_single_index();

        if defer {
            match strategy {
                CandidateStrategy::UseIndex => {
                    if let CandidatePlan::Narrowed { file_ids } = index_plan {
                        return Ok(ResolvedCandidates::Indexed(IndexedCandidates::from_ids(
                            file_ids,
                            self.source.indexes,
                            matching,
                        )));
                    }
                }
                CandidateStrategy::AllIndexed => {
                    if let Some(count) = self.source.indexes.primary_file_count() {
                        return Ok(ResolvedCandidates::Indexed(IndexedCandidates::all(
                            count,
                            self.source.indexes,
                            matching,
                        )));
                    }
                }
                CandidateStrategy::None
                | CandidateStrategy::Walk
                | CandidateStrategy::MergeIndexAndWalk => {}
            }
        }

        let (raw, filtered) = match strategy {
            CandidateStrategy::None => (Vec::new(), true),
            CandidateStrategy::Walk => (
                FileWalk::from_filter(self.source.filter).candidates()?,
                false,
            ),
            CandidateStrategy::AllIndexed => {
                (self.source.indexes.all_indexed_candidates(matching), true)
            }
            CandidateStrategy::UseIndex => match index_plan {
                CandidatePlan::Narrowed { file_ids } => {
                    (self.source.indexes.materialize(&file_ids, matching), true)
                }
                CandidatePlan::AllIndexed | CandidatePlan::Unavailable => (Vec::new(), true),
            },
            CandidateStrategy::MergeIndexAndWalk => (self.merge_unindexed(index_plan)?, false),
        };
        let set = CandidateSet::new(raw);
        let set = if filtered {
            set
        } else {
            set.retain_matches(self.source.filter, admission)
        };
        Ok(ResolvedCandidates::Ready(set.order(order)?.into_vec()))
    }

    const fn index_status(&self, index_plan: &CandidatePlan) -> IndexStatus {
        if self.source.indexes.is_empty() {
            IndexStatus::Empty
        } else {
            match index_plan {
                CandidatePlan::Unavailable => IndexStatus::NoCandidateIndex,
                CandidatePlan::AllIndexed => IndexStatus::AllCandidates,
                CandidatePlan::Narrowed { .. } => IndexStatus::CanNarrow,
            }
        }
    }

    fn snapshot_status(&self) -> SnapshotStatus {
        let Some(meta) = self.source.store_meta else {
            return SnapshotStatus::Missing;
        };
        if !meta.covers_candidate_filter(self.source.filter) {
            return SnapshotStatus::FilterMismatch;
        }
        match meta.coverage {
            IndexCoverage::Complete if self.request.fallback.walk_on_stale() => {
                SnapshotStatus::StaleComplete
            }
            IndexCoverage::Complete => SnapshotStatus::TrustedComplete,
            IndexCoverage::Lazy => SnapshotStatus::TrustedLazy,
        }
    }

    fn merge_unindexed(&self, index_plan: CandidatePlan) -> crate::Result<Vec<Candidate>> {
        let CandidatePlan::Narrowed { file_ids } = index_plan else {
            return FileWalk::from_filter(self.source.filter).candidates();
        };
        let mut candidates = self
            .source
            .indexes
            .materialize(&file_ids, MaterializeRequest::All);

        let walked = self
            .source
            .indexes
            .unindexed_candidates(self.source.filter)?;
        let mut seen: HashSet<PathBuf> = candidates
            .iter()
            .map(|candidate| candidate.rel_path().to_path_buf())
            .collect();
        for candidate in walked {
            if seen.insert(candidate.rel_path().to_path_buf()) {
                candidates.push(candidate);
            }
        }
        Ok(candidates)
    }
}

const fn plan(input: PlanInput) -> CandidateStrategy {
    match input.scope {
        CandidateScope::None => CandidateStrategy::None,
        CandidateScope::All => plan_all(input),
        CandidateScope::Indexed => plan_indexed(input),
    }
}

const fn plan_all(input: PlanInput) -> CandidateStrategy {
    match (input.index_status, input.snapshot_status, input.fallback) {
        (IndexStatus::Empty, _, _) => CandidateStrategy::Walk,
        (_, SnapshotStatus::FilterMismatch | SnapshotStatus::TrustedLazy, _)
        | (_, SnapshotStatus::StaleComplete, IndexFallback::WalkOnStaleSnapshot) => {
            CandidateStrategy::Walk
        }
        (_, _, _) => CandidateStrategy::AllIndexed,
    }
}

const fn plan_indexed(input: PlanInput) -> CandidateStrategy {
    match (input.index_status, input.index_narrowing, input.fallback) {
        (IndexStatus::Empty, _, _)
        | (_, IndexNarrowing::Disabled, _)
        | (IndexStatus::NoCandidateIndex, _, IndexFallback::WalkOnStaleSnapshot) => {
            CandidateStrategy::Walk
        }
        (
            IndexStatus::NoCandidateIndex | IndexStatus::AllCandidates,
            _,
            IndexFallback::IndexHitsOnly,
        ) => CandidateStrategy::AllIndexed,
        (IndexStatus::AllCandidates, _, IndexFallback::WalkOnStaleSnapshot) => {
            match input.snapshot_status {
                SnapshotStatus::Missing | SnapshotStatus::TrustedComplete => {
                    CandidateStrategy::AllIndexed
                }
                SnapshotStatus::FilterMismatch
                | SnapshotStatus::TrustedLazy
                | SnapshotStatus::StaleComplete => CandidateStrategy::Walk,
            }
        }
        (IndexStatus::CanNarrow, _, _) => match input.snapshot_status {
            SnapshotStatus::TrustedLazy => CandidateStrategy::MergeIndexAndWalk,
            _ => CandidateStrategy::UseIndex,
        },
    }
}

impl CandidateStrategy {
    const fn filter_admission(self, snapshot_status: SnapshotStatus) -> FilterAdmission {
        match (self, snapshot_status) {
            (
                Self::UseIndex | Self::AllIndexed,
                SnapshotStatus::TrustedComplete
                | SnapshotStatus::TrustedLazy
                | SnapshotStatus::StaleComplete,
            ) => FilterAdmission::Indexed,
            _ => FilterAdmission::Full,
        }
    }
}

impl CandidateSet {
    const fn new(candidates: Vec<Candidate>) -> Self {
        Self { candidates }
    }

    fn retain_matches(
        mut self,
        filter: &crate::corpus::filter::CandidateFilter,
        admission: FilterAdmission,
    ) -> Self {
        self.candidates = self
            .candidates
            .into_par_iter()
            .filter(|candidate| candidate.matches(filter, admission))
            .collect();
        self
    }

    fn order(mut self, order: crate::corpus::CandidateOrder) -> crate::Result<Self> {
        order.order(&mut self.candidates)?;
        Ok(self)
    }

    fn into_vec(self) -> Vec<Candidate> {
        self.candidates
    }
}
