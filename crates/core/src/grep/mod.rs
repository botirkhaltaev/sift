//! Grep pipeline orchestration.
//!
//! Bridges the query planner, index registry, candidate filter, and search
//! engine into grep search operations. The pipeline is
//! index-agnostic: it works with whatever index types the `Indexes` registry
//! has opened.

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
pub enum GrepSource<'a> {
    /// Resolve and search the configured corpus candidates.
    Corpus,
    /// Search named byte streams without resolving corpus candidates.
    Streams(&'a [StreamInput<'a>]),
}

/// User-facing request to the grep pipeline.
pub struct GrepRequest<'a> {
    pub indexes: &'a Indexes,
    pub filter: &'a CandidateFilter,
    pub output: SearchOutput,
    pub separators: &'a SearchSeparators,
    pub collect: SearchCollection,
    pub candidate_source: CandidateSource<'a>,
}

impl GrepRequest<'_> {
    /// Search one or more source kinds as a single grep execution.
    ///
    /// # Errors
    ///
    /// Returns an error if candidate resolution, regex compilation, or search execution fails.
    pub fn search(
        &self,
        query: &SearchQuery,
        sources: &[GrepSource<'_>],
    ) -> crate::Result<GrepRun> {
        Self::validate_query(query)?;

        let mut corpus_requested = false;
        let mut streams = Vec::new();
        for source in sources {
            match *source {
                GrepSource::Corpus => corpus_requested = true,
                GrepSource::Streams(source_streams) => streams.extend_from_slice(source_streams),
            }
        }

        let candidates = if corpus_requested {
            self.resolve_candidates(query)?
        } else {
            Vec::new()
        };

        let mut inputs = Vec::with_capacity(usize::from(!candidates.is_empty()) + streams.len());
        if !candidates.is_empty() {
            inputs.push(SearchInput::Candidates(&candidates));
        }
        inputs.extend(streams.into_iter().map(SearchInput::Stream));

        self.search_inputs(query, inputs)
    }

    fn resolve_candidates(&self, query: &SearchQuery) -> crate::Result<Vec<Candidate>> {
        let spec = query.build_query_spec();
        let output = self.output;
        let requirement = if query.opts().precludes_trigram_index() {
            crate::query::CandidateRequirement::Complete
        } else {
            output.candidate_requirement()
        };

        let raw = QueryPlanner::new(spec).candidates(
            CandidatePlan {
                indexes: self.indexes,
                requirement,
                filter: self.filter,
                source: self.candidate_source,
            },
            || self.filter.collect(),
        )?;

        Ok(raw
            .into_par_iter()
            .filter(|c| c.matches(self.filter))
            .collect())
    }

    fn search_inputs(
        &self,
        query: &SearchQuery,
        inputs: Vec<SearchInput<'_>>,
    ) -> crate::Result<GrepRun> {
        if inputs.is_empty() {
            return Ok(self.empty_run());
        }

        let (outcome, hits) = query.search(&SearchExecution {
            inputs,
            output: self.output,
            separators: self.separators,
            collect: self.collect.with_hits(true),
        })?;

        Ok(GrepRun { outcome, hits })
    }

    fn validate_query(query: &SearchQuery) -> crate::Result<()> {
        if query.opts().max_results == Some(0) {
            return Err(crate::Error::Search(SearchError::InvalidMaxCount));
        }

        Ok(())
    }

    fn empty_run(&self) -> GrepRun {
        GrepRun {
            outcome: SearchOutcome {
                matched: false,
                stats: self.collect.stats.then_some(SearchStats::default()),
            },
            hits: Vec::new(),
        }
    }
}
