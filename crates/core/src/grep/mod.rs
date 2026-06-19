//! Grep pipeline orchestration.
//!
//! Bridges the logical query, index planner, and candidate filter.
//! This is the primary API the CLI calls.

use std::path::PathBuf;

use crate::Candidate;
use crate::CandidateFilter;
use crate::SearchOutput;
use crate::SearchQuery;
use crate::SearchSeparators;
use crate::SearchStats;
use crate::index::Indexes;
use crate::query::QueryPlanner;
use crate::search::SearchError;
use crate::search::SearchOutcome;
use crate::search::request::{SearchCollection, SearchExecution};
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
    pub walk_unindexed: bool,
}

impl GrepRequest<'_> {
    /// Run the full grep pipeline: resolve candidates, execute search, return outcome.
    ///
    /// # Errors
    ///
    /// Returns an error if candidate resolution, regex compilation, or search execution fails.
    pub fn run(&self, query: &SearchQuery) -> crate::Result<GrepRun> {
        if query.opts.max_results == Some(0) {
            return Err(crate::Error::Search(SearchError::InvalidMaxCount));
        }

        let spec = query.spec();
        let output = self.output;
        let requirement = output.candidate_requirement();

        let raw = QueryPlanner::new(spec).candidates(
            self.indexes,
            requirement,
            self.filter,
            self.store_meta,
            self.walk_unindexed,
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
            candidates,
            output,
            separators: self.separators,
            collect: self.collect.with_hits(true),
        })?;

        Ok(GrepRun { outcome, hits })
    }
}
