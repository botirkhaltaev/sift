use std::collections::HashSet;
use std::path::PathBuf;

use rayon::prelude::*;

use crate::IndexCoverage;
use crate::candidates::{
    CandidateRequest, CandidateScope, CandidateSource, CandidateSpec, IndexFallback,
};
use crate::corpus::Candidate;
use crate::corpus::walk::{FileWalk, WalkMetadata};
use crate::index::CandidatePlan;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IndexNarrowing {
    Enabled,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IndexStatus {
    Empty,
    NoCandidateIndex,
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
    pub fn resolve(self) -> crate::Result<Vec<Candidate>> {
        let index_narrowing = self.spec.index_narrowing();
        let resolved = self.request.resolve(index_narrowing);
        let index_plan = self.source.indexes.plan(&self.spec);
        let strategy = plan(PlanInput {
            scope: resolved.scope,
            index_narrowing,
            index_status: self.index_status(&index_plan),
            snapshot_status: self.snapshot_status(),
            fallback: resolved.fallback,
        });
        self.execute(strategy, index_plan, resolved.order)
    }

    fn execute(
        self,
        strategy: CandidateStrategy,
        index_plan: CandidatePlan,
        order: crate::corpus::CandidateOrder,
    ) -> crate::Result<Vec<Candidate>> {
        let raw = match strategy {
            CandidateStrategy::None => Vec::new(),
            CandidateStrategy::Walk => FileWalk::from_filter(self.source.filter)
                .metadata(WalkMetadata::Read)
                .files()?
                .into_iter()
                .map(crate::corpus::walk::WalkFile::into_candidate)
                .collect(),
            CandidateStrategy::AllIndexed => self.source.indexes.complete_candidates(),
            CandidateStrategy::UseIndex => match index_plan {
                CandidatePlan::Narrowed { candidates, .. } => candidates,
                CandidatePlan::AllIndexed { .. } | CandidatePlan::Unavailable => Vec::new(),
            },
            CandidateStrategy::MergeIndexAndWalk => self.merge_unindexed(index_plan)?,
        };
        Ok(CandidateSet::new(raw)
            .retain_matches(self.source.filter)
            .order(order)?
            .into_vec())
    }

    const fn index_status(&self, index_plan: &CandidatePlan) -> IndexStatus {
        if self.source.indexes.is_empty() {
            IndexStatus::Empty
        } else {
            match index_plan {
                CandidatePlan::Unavailable => IndexStatus::NoCandidateIndex,
                CandidatePlan::AllIndexed { .. } | CandidatePlan::Narrowed { .. } => {
                    IndexStatus::CanNarrow
                }
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
        let CandidatePlan::Narrowed { mut candidates, .. } = index_plan else {
            return Ok(FileWalk::from_filter(self.source.filter)
                .metadata(WalkMetadata::Read)
                .files()?
                .into_iter()
                .map(crate::corpus::walk::WalkFile::into_candidate)
                .collect());
        };

        let walked = self
            .source
            .indexes
            .unindexed_walk_paths(self.source.filter)?;
        let mut seen: HashSet<PathBuf> = candidates
            .iter()
            .map(|candidate| candidate.rel_path().to_path_buf())
            .collect();
        for rel_path in walked {
            if seen.insert(rel_path.clone()) {
                let abs_path = self.source.filter.root().join(&rel_path);
                let size = std::fs::metadata(&abs_path)
                    .ok()
                    .map(|metadata| metadata.len());
                let depth = Some(rel_path.components().count().saturating_sub(1));
                let candidate = Candidate::with_metadata(rel_path, abs_path, size, depth);
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
        (IndexStatus::NoCandidateIndex, _, IndexFallback::IndexHitsOnly) => {
            CandidateStrategy::AllIndexed
        }
        (IndexStatus::CanNarrow, _, _) => match input.snapshot_status {
            SnapshotStatus::TrustedLazy => CandidateStrategy::MergeIndexAndWalk,
            _ => CandidateStrategy::UseIndex,
        },
    }
}

impl CandidateSet {
    const fn new(candidates: Vec<Candidate>) -> Self {
        Self { candidates }
    }

    fn retain_matches(mut self, filter: &crate::corpus::filter::CandidateFilter) -> Self {
        self.candidates = self
            .candidates
            .into_par_iter()
            .filter(|candidate| candidate.matches(filter))
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
