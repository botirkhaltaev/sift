use std::collections::HashSet;
use std::path::PathBuf;

use crate::Candidate;
use crate::StoreMeta;
use crate::index::Indexes;
use crate::query::QuerySpec;
use crate::search::CandidateFilter;

/// Whether search needs all candidate paths or only potential matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateRequirement {
    Complete,
    PotentialMatches,
}

/// Whether the planner should walk for files not present in the index snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UnindexedStrategy {
    /// Return only index-narrowed candidates. No filesystem walk.
    #[default]
    Skip,
    /// Walk to discover files not yet indexed and merge them into results.
    Walk,
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
    /// When `unindexed` is [`UnindexedStrategy::Walk`] and the index narrows
    /// candidates, also walk corpus paths that are not yet present in the
    /// current snapshot.
    ///
    /// # Errors
    ///
    /// Delegates to `base` when fallback is triggered; returns `base` errors unchanged.
    pub fn candidates(
        &self,
        indexes: &Indexes,
        requirement: CandidateRequirement,
        filter: &CandidateFilter,
        store_meta: Option<&StoreMeta>,
        unindexed: UnindexedStrategy,
        base: impl FnOnce() -> crate::Result<Vec<Candidate>>,
    ) -> crate::Result<Vec<Candidate>> {
        match requirement {
            CandidateRequirement::Complete => {
                if indexes.is_empty() {
                    return base();
                }
                if store_meta.is_some_and(|meta| !meta.matches_search_filter(filter)) {
                    return base();
                }
                Ok(indexes.complete_candidates())
            }
            CandidateRequirement::PotentialMatches => {
                if indexes.is_empty() {
                    return base();
                }
                match indexes.candidates(&self.spec) {
                    None => base(),
                    Some(snapshot_hits) if unindexed == UnindexedStrategy::Skip => {
                        Ok(snapshot_hits)
                    }
                    Some(mut snapshot_hits) => {
                        let indexed_paths = indexes.indexed_rel_paths();
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
            }
        }
    }
}
