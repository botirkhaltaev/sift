//! Grep pipeline orchestration.
//!
//! Bridges the query planner, index registry, candidate filter, and search
//! engine into a single `GrepRequest::run()` call. This is the primary API
//! the CLI calls. The pipeline is index-agnostic: it works with whatever
//! index types the `Indexes` registry has opened.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::Candidate;
use crate::index::Indexes;
use crate::query::{CandidatePlan, CandidateSource, QueryPlanner};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CandidateSortKey {
    #[default]
    None,
    Path,
    Modified,
    Accessed,
    Created,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CandidateSort {
    pub key: CandidateSortKey,
    pub reverse: bool,
}

impl CandidateSort {
    #[must_use]
    pub const fn new(key: CandidateSortKey, reverse: bool) -> Self {
        Self { key, reverse }
    }

    #[must_use]
    pub const fn is_sorted(self) -> bool {
        !matches!(self.key, CandidateSortKey::None)
    }

    /// Sort candidates in place according to the configured key.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when filesystem metadata required by a timestamp
    /// sort key cannot be read.
    pub fn sort_candidates(self, candidates: &mut [Candidate]) -> crate::Result<()> {
        match self.key {
            CandidateSortKey::None => {}
            CandidateSortKey::Path => candidates.sort_by(|a, b| a.rel_path().cmp(b.rel_path())),
            CandidateSortKey::Modified => Self::sort_by_time(candidates, |path| {
                std::fs::metadata(path).and_then(|meta| meta.modified())
            })?,
            CandidateSortKey::Accessed => Self::sort_by_time(candidates, |path| {
                std::fs::metadata(path).and_then(|meta| meta.accessed())
            })?,
            CandidateSortKey::Created => Self::sort_by_time(candidates, |path| {
                std::fs::metadata(path).and_then(|meta| meta.created())
            })?,
        }
        if self.reverse {
            candidates.reverse();
        }
        Ok(())
    }

    fn sort_by_time(
        candidates: &mut [Candidate],
        timestamp: impl Fn(&Path) -> std::io::Result<SystemTime>,
    ) -> crate::Result<()> {
        let mut keyed = Vec::with_capacity(candidates.len());
        for candidate in candidates.iter().cloned() {
            let time = timestamp(candidate.abs_path())?;
            keyed.push((time, candidate.rel_path().to_path_buf(), candidate));
        }
        keyed.sort_by_key(|(time, path, _)| (*time, path.clone()));
        for (slot, (_, _, candidate)) in candidates.iter_mut().zip(keyed) {
            *slot = candidate;
        }
        Ok(())
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
    pub candidate_sort: CandidateSort,
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
            CandidatePlan {
                indexes: self.indexes,
                requirement,
                filter: self.filter,
                source: self.candidate_source,
            },
            || self.filter.collect(),
        )?;

        let mut candidates: Vec<Candidate> = raw
            .into_par_iter()
            .filter(|c| c.matches(self.filter))
            .collect();
        self.candidate_sort.sort_candidates(&mut candidates)?;

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
