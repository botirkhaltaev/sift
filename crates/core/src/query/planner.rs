use crate::corpus::CandidateCoverage;
use crate::corpus::filter::CandidateFilter;
use crate::index::Indexes;
use crate::query::{ResolutionPlan, QuerySpec};
use crate::{IndexCoverage, StoreMeta};

/// Inputs for pure candidate planning.
#[derive(Clone, Copy)]
pub struct PlanContext<'a> {
    pub indexes: &'a Indexes,
    pub filter: &'a CandidateFilter,
    pub store_meta: Option<&'a StoreMeta>,
    /// Whether the query can be narrowed by the index layer.
    pub index_capable: bool,
}

/// Plans candidate selection without performing I/O.
pub struct QueryPlanner<'a> {
    pub(crate) spec: QuerySpec<'a>,
}

impl<'a> PlanContext<'a> {
    #[must_use]
    pub const fn new(
        indexes: &'a Indexes,
        filter: &'a CandidateFilter,
        store_meta: Option<&'a StoreMeta>,
        index_capable: bool,
    ) -> Self {
        Self {
            indexes,
            filter,
            store_meta,
            index_capable,
        }
    }
}

impl<'a> QueryPlanner<'a> {
    #[must_use]
    pub const fn new(spec: QuerySpec<'a>) -> Self {
        Self { spec }
    }

    /// Decide how candidates should be resolved for this query.
    #[must_use]
    pub fn plan(
        self,
        ctx: PlanContext<'_>,
        coverage: CandidateCoverage,
        walk_on_stale: bool,
    ) -> ResolutionPlan {
        let strategy = match coverage {
            CandidateCoverage::Complete => Self::plan_complete(ctx, walk_on_stale),
            CandidateCoverage::PotentialMatches => self.plan_potential(ctx, walk_on_stale),
        };
        ResolutionPlan { strategy }
    }

    fn plan_complete(ctx: PlanContext<'_>, walk_on_stale: bool) -> super::plan::ResolutionStrategy {
        use super::plan::ResolutionStrategy;
        if ctx.indexes.is_empty() {
            return ResolutionStrategy::WalkAll;
        }
        if ctx
            .store_meta
            .is_some_and(|meta| !meta.covers_candidate_filter(ctx.filter))
        {
            return ResolutionStrategy::WalkAll;
        }
        if ctx
            .store_meta
            .is_some_and(|meta| meta.coverage == IndexCoverage::Lazy)
        {
            return ResolutionStrategy::WalkAll;
        }
        if walk_on_stale {
            return ResolutionStrategy::WalkAll;
        }
        ResolutionStrategy::AllIndexed
    }

    fn plan_potential(
        self,
        ctx: PlanContext<'_>,
        walk_on_stale: bool,
    ) -> super::plan::ResolutionStrategy {
        use super::plan::ResolutionStrategy;
        if ctx.indexes.is_empty() || !ctx.index_capable {
            return ResolutionStrategy::WalkAll;
        }
        if ctx.indexes.candidates(&self.spec).is_none() {
            return if walk_on_stale {
                ResolutionStrategy::WalkAll
            } else {
                ResolutionStrategy::AllIndexed
            };
        }
        ResolutionStrategy::UseIndex
    }
}
