//! Grep pipeline orchestration.
//!
//! Bridges the query planner, index registry, candidate filter, and search
//! engine into a single `GrepRequest::run()` call. This is the primary API
//! the CLI calls. The pipeline is index-agnostic: it works with whatever
//! index types the `Indexes` registry has opened.

use std::path::PathBuf;

use crate::Candidate;
use crate::index::Indexes;
use crate::query::{CandidatePlan, CandidateSource, QueryPlanner};
use crate::search::request::{SearchCollection, SearchExecution, SearchInput, StreamInput};
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

#[derive(Clone, Copy)]
pub enum GrepInput<'a> {
    Candidates,
    Stream(StreamInput<'a>),
    CandidatesAndStream(StreamInput<'a>),
}

impl<'a> GrepInput<'a> {
    const fn needs_candidates(self) -> bool {
        matches!(self, Self::Candidates | Self::CandidatesAndStream(_))
    }

    fn search_inputs(self, candidates: &'a [Candidate]) -> Vec<SearchInput<'a>> {
        match self {
            Self::Candidates => {
                if candidates.is_empty() {
                    Vec::new()
                } else {
                    vec![SearchInput::Candidates(candidates)]
                }
            }
            Self::Stream(stream) => vec![SearchInput::Stream(stream)],
            Self::CandidatesAndStream(stream) => {
                if candidates.is_empty() {
                    vec![SearchInput::Stream(stream)]
                } else {
                    vec![
                        SearchInput::Candidates(candidates),
                        SearchInput::Stream(stream),
                    ]
                }
            }
        }
    }
}

/// User-facing request to the grep pipeline.
pub struct GrepRequest<'a> {
    pub indexes: &'a Indexes,
    pub filter: &'a CandidateFilter,
    pub output: SearchOutput,
    pub separators: &'a SearchSeparators,
    pub collect: SearchCollection,
    pub candidate_source: CandidateSource<'a>,
    pub input: GrepInput<'a>,
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
        let requirement = if query.opts().precludes_trigram_index() {
            crate::query::CandidateRequirement::Complete
        } else {
            output.candidate_requirement()
        };

        let raw = if self.input.needs_candidates() {
            QueryPlanner::new(spec).candidates(
                CandidatePlan {
                    indexes: self.indexes,
                    requirement,
                    filter: self.filter,
                    source: self.candidate_source,
                },
                || self.filter.collect(),
            )?
        } else {
            Vec::new()
        };

        let candidates: Vec<Candidate> = raw
            .into_par_iter()
            .filter(|c| c.matches(self.filter))
            .collect();

        let inputs = self.input.search_inputs(&candidates);
        if inputs.is_empty() {
            return Ok(GrepRun {
                outcome: SearchOutcome {
                    matched: false,
                    stats: self.collect.stats.then_some(SearchStats::default()),
                },
                hits: Vec::new(),
            });
        }

        let (outcome, hits) = query.search(&SearchExecution {
            inputs,
            output,
            separators: self.separators,
            collect: self.collect.with_hits(true),
        })?;

        Ok(GrepRun { outcome, hits })
    }
}
