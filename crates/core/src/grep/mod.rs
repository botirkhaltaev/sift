//! Grep pipeline orchestration.
//!
//! Bridges the logical query, index planner, and candidate filter.
//! This is the primary API the CLI calls.

use crate::Candidate;
use crate::CandidateFilter;
use crate::SearchOutput;
use crate::SearchQuery;
use crate::SearchSeparators;
use crate::SearchStats;
use crate::index::Indexes;
use crate::search::SearchError;
use crate::search::SearchOutcome;
use crate::search::candidates::walk;
use crate::search::request::SearchExecution;
use rayon::prelude::*;

/// User-facing request to the grep pipeline.
pub struct GrepRequest<'a> {
    pub indexes: &'a Indexes,
    pub filter: &'a CandidateFilter,
    pub output: SearchOutput,
    pub separators: &'a SearchSeparators,
    pub collect_stats: bool,
}

/// Run the full grep pipeline: resolve candidates, execute search, return outcome.
///
/// # Errors
///
/// Returns an error if candidate resolution, regex compilation, or search execution fails.
pub fn run(query: &SearchQuery, request: &GrepRequest<'_>) -> crate::Result<SearchOutcome> {
    if query.opts.max_results == Some(0) {
        return Err(crate::Error::Search(SearchError::InvalidMaxCount));
    }

    let spec = query.spec();
    let output = request.output;

    let raw = if request.indexes.is_empty() {
        walk::collect_candidates(request.filter)?
    } else {
        let coverage = output.candidate_coverage();
        request.indexes.candidates(&spec, coverage)
    };

    let candidates: Vec<Candidate> = raw
        .into_par_iter()
        .filter(|c| c.matches(request.filter))
        .collect();

    if candidates.is_empty() {
        return Ok(SearchOutcome {
            matched: false,
            stats: request.collect_stats.then_some(SearchStats::default()),
        });
    }

    query.search(&SearchExecution {
        candidates,
        output,
        separators: request.separators,
        collect_stats: request.collect_stats,
    })
}
