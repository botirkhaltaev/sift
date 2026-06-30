use std::collections::HashSet;
use std::path::PathBuf;

use crate::Candidate;
use crate::grep::CandidateFilter;
use crate::index::Indexes;
use crate::query::QuerySpec;
use crate::{IndexCoverage, StoreMeta};

/// Whether search needs all candidate paths or only potential matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateRequirement {
    Complete,
    PotentialMatches,
}

/// Whether the opened snapshot has been validated as a complete read version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SnapshotValidation {
    #[default]
    Unvalidated,
    Validated,
    Stale,
}

/// Candidate source policy for a grep run.
#[derive(Clone, Copy)]
pub struct CandidateSource<'a> {
    pub store_meta: Option<&'a StoreMeta>,
    pub snapshot: SnapshotValidation,
}

/// Inputs for candidate planning.
#[derive(Clone, Copy)]
pub struct CandidatePlan<'a> {
    pub indexes: &'a Indexes,
    pub requirement: CandidateRequirement,
    pub filter: &'a CandidateFilter,
    pub source: CandidateSource<'a>,
}

/// Plans candidate selection by consulting the index registry and falling back
/// to a filesystem walk when no index can narrow the query.
///
/// The planner is the single coordination point between the search pipeline
/// and the index layer. It is index-agnostic: it calls `Indexes::candidates()`
/// without knowing which index types are present.
pub struct QueryPlanner<'a> {
    spec: QuerySpec<'a>,
}

impl<'a> QueryPlanner<'a> {
    #[must_use]
    pub const fn new(spec: QuerySpec<'a>) -> Self {
        Self { spec }
    }

    /// Resolve candidates using indexes or the lazy base provider.
    ///
    /// Lazy stores may merge filesystem walk results for paths not present in
    /// the current snapshot. Complete stores use snapshot candidates unless an
    /// explicit stale validation result requires a conservative walk fallback.
    ///
    /// # Errors
    ///
    /// Delegates to `base` when fallback is triggered; returns `base` errors unchanged.
    pub fn candidates(
        &self,
        plan: CandidatePlan<'_>,
        base: impl FnOnce() -> crate::Result<Vec<Candidate>>,
    ) -> crate::Result<Vec<Candidate>> {
        match plan.requirement {
            CandidateRequirement::Complete => {
                if plan.indexes.is_empty() {
                    return base();
                }
                if plan
                    .source
                    .store_meta
                    .is_some_and(|meta| !meta.covers_candidate_filter(plan.filter))
                {
                    return base();
                }
                if plan
                    .source
                    .store_meta
                    .is_some_and(|meta| meta.coverage == IndexCoverage::Lazy)
                {
                    return base();
                }
                Ok(plan.indexes.complete_candidates())
            }
            CandidateRequirement::PotentialMatches => {
                if plan.indexes.is_empty() {
                    return base();
                }
                match plan.indexes.candidates(&self.spec) {
                    None => base(),
                    Some(snapshot_hits) => Self::resolve_index_hits(plan, snapshot_hits, base),
                }
            }
        }
    }

    fn resolve_index_hits(
        plan: CandidatePlan<'_>,
        snapshot_hits: Vec<Candidate>,
        base: impl FnOnce() -> crate::Result<Vec<Candidate>>,
    ) -> crate::Result<Vec<Candidate>> {
        let Some(meta) = plan.source.store_meta else {
            return Ok(snapshot_hits);
        };

        if !meta.covers_candidate_filter(plan.filter) {
            return base();
        }

        match meta.coverage {
            IndexCoverage::Complete => {
                if plan.source.snapshot == SnapshotValidation::Stale {
                    base()
                } else {
                    Ok(snapshot_hits)
                }
            }
            IndexCoverage::Lazy => Self::merge_unindexed(plan, snapshot_hits, base),
        }
    }

    fn merge_unindexed(
        plan: CandidatePlan<'_>,
        mut snapshot_hits: Vec<Candidate>,
        base: impl FnOnce() -> crate::Result<Vec<Candidate>>,
    ) -> crate::Result<Vec<Candidate>> {
        let indexed_paths = plan.indexes.indexed_rel_paths();
        let walked = base()?;
        let mut seen: HashSet<PathBuf> = snapshot_hits
            .iter()
            .map(|c| c.rel_path().to_path_buf())
            .collect();
        for candidate in walked {
            if indexed_paths.contains(candidate.rel_path()) {
                continue;
            }
            if seen.insert(candidate.rel_path().to_path_buf()) {
                snapshot_hits.push(candidate);
            }
        }
        Ok(snapshot_hits)
    }
}
