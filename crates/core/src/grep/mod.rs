//! Grep pipeline orchestration.
//!
//! Bridges the query planner, index registry, candidate filter, and search
//! engine into a single `GrepRequest::run()` call. This is the primary API
//! the CLI calls. The pipeline is index-agnostic: it works with whatever
//! index types the `Indexes` registry has opened.

use std::path::PathBuf;

use crate::Candidate;
use crate::index::Indexes;
use crate::query::{QueryPlanner, UnindexedStrategy};
use crate::search::request::{SearchCollection, SearchExecution};
use crate::search::{
    CandidateFilter, SearchError, SearchOutcome, SearchOutput, SearchQuery, SearchSeparators,
    SearchStats,
};
use rayon::prelude::*;

/// Result of the grep pipeline.
pub struct GrepRun {
    pub outcome: SearchOutcome,
    /// Unique rel-paths with at least one pattern hit.
    pub hits: Vec<PathBuf>,
}

/// User-facing request to the grep pipeline.
pub struct GrepRequest<'a> {
    pub indexes: &'a Indexes,
    pub filter: &'a CandidateFilter,
    pub output: SearchOutput,
    pub separators: &'a SearchSeparators,
    pub collect: SearchCollection,
    pub store_meta: Option<&'a crate::StoreMeta>,
    pub unindexed: UnindexedStrategy,
}

impl GrepRequest<'_> {
    /// Run the full grep pipeline: resolve candidates, execute search, return outcome.
    ///
    /// # Errors
    ///
    /// Returns an error if candidate resolution, regex compilation, or search execution fails.
    pub fn run(&self, query: &SearchQuery) -> crate::Result<GrepRun> {
        if query.opts().max_results == Some(0) {
            return Err(crate::Error::Search(SearchError::InvalidMaxCount));
        }

        let spec = query.build_query_spec();
        let output = self.output;
        let requirement = output.candidate_requirement();

        let raw = QueryPlanner::new(spec).candidates(
            self.indexes,
            requirement,
            self.filter,
            self.store_meta,
            self.unindexed,
            || self.filter.collect(),
        )?;

        let candidates: Vec<Candidate> = raw
            .into_par_iter()
            .filter(|c| c.matches(self.filter))
            .collect();

        if candidates.is_empty() {
            return Ok(GrepRun {
                outcome: SearchOutcome {
                    matched: false,
                    stats: self.collect.stats.then_some(SearchStats::default()),
                },
                hits: Vec::new(),
            });
        }

        let (outcome, hits) = query.search(&SearchExecution {
            candidates: &candidates,
            output,
            separators: self.separators,
            collect: self.collect.with_hits(true),
        })?;

        Ok(GrepRun { outcome, hits })
    }
}
