//! Grep pipeline orchestration.
//!
//! Bridges the query planner, index registry, candidate filter, and search
//! engine into corpus and stream search operations. The pipeline is
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

impl GrepRun {
    pub fn merge(&mut self, other: Self) {
        self.outcome.matched |= other.outcome.matched;
        merge_stats(&mut self.outcome.stats, other.outcome.stats);
        self.hits.extend(other.hits);
        self.hits.sort();
        self.hits.dedup();
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
}

impl GrepRequest<'_> {
    /// Search the configured corpus by resolving file candidates first.
    ///
    /// # Errors
    ///
    /// Returns an error if candidate resolution, regex compilation, or search execution fails.
    pub fn search_corpus(&self, query: &SearchQuery) -> crate::Result<GrepRun> {
        let candidates = self.resolve_candidates(query)?;
        if candidates.is_empty() {
            return Ok(self.empty_run());
        }

        self.search_inputs(query, vec![SearchInput::Candidates(&candidates)])
    }

    /// Search named byte streams without resolving corpus candidates.
    ///
    /// # Errors
    ///
    /// Returns an error if regex compilation or search execution fails.
    pub fn search_streams(
        &self,
        query: &SearchQuery,
        streams: &[StreamInput<'_>],
    ) -> crate::Result<GrepRun> {
        Self::validate_query(query)?;
        if streams.is_empty() {
            return Ok(self.empty_run());
        }

        self.search_inputs(
            query,
            streams.iter().copied().map(SearchInput::Stream).collect(),
        )
    }

    fn resolve_candidates(&self, query: &SearchQuery) -> crate::Result<Vec<Candidate>> {
        Self::validate_query(query)?;

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

fn merge_stats(stats: &mut Option<SearchStats>, other: Option<SearchStats>) {
    match (stats, other) {
        (Some(stats), Some(other)) => {
            stats.matches += other.matches;
            stats.files_with_matches += other.files_with_matches;
            stats.files_searched += other.files_searched;
            stats.bytes_printed += other.bytes_printed;
            stats.bytes_searched += other.bytes_searched;
            stats.elapsed += other.elapsed;
        }
        (stats @ None, other) => *stats = other,
        (Some(_), None) => {}
    }
}
