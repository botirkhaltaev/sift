//! Grep pipeline orchestration.
//!
//! Bridges the logical query, index planner, and search executor.
//! This is the primary API the CLI calls.

use crate::CandidateInfo;
use crate::CandidateSet;
use crate::SearchFilter;
use crate::SearchOutput;
use crate::SearchQuery;
use crate::SearchSeparators;
use crate::SearchStats;
use crate::index::Indexes;
use crate::search::SearchError;
use crate::search::SearchOutcome;
use crate::search::candidates::{indexed, walk};
use crate::search::request::SearchExecution;

/// User-facing request to the grep pipeline.
pub struct GrepRequest<'a> {
    pub indexes: &'a Indexes,
    pub filter: &'a SearchFilter,
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

    let candidates = if request.indexes.is_empty() {
        resolve_walk_candidates(request.filter)?
    } else {
        resolve_indexed_candidates(request.indexes, &spec, output, request.filter)
    };

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

fn resolve_walk_candidates(filter: &SearchFilter) -> crate::Result<Vec<CandidateInfo>> {
    let abs_paths = walk::collect_abs_paths_for_scopes(filter)?;
    if abs_paths.is_empty() {
        return Ok(Vec::new());
    }
    Ok(walk::prepare_walk_candidates(&abs_paths, filter))
}

fn resolve_indexed_candidates(
    indexes: &Indexes,
    spec: &crate::query::QuerySpec<'_>,
    output: SearchOutput,
    filter: &SearchFilter,
) -> Vec<CandidateInfo> {
    let raw = match output.candidate_set() {
        CandidateSet::AllIndexedFiles => indexes.resolve_all_files(),
        CandidateSet::IndexedCandidates => indexes.resolve_candidates(spec),
    };
    indexed::prepare_candidates(raw, filter)
}
