//! Grep pipeline orchestration.
//!
//! Bridges the query planner, index registry, candidate filter, and search
//! engine into grep search operations. The pipeline is
//! index-agnostic: it works with whatever index types the `Indexes` registry
//! has opened.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::Candidate;
use crate::index::Indexes;
use crate::query::{CandidatePlan, CandidateRequirement, CandidateSource, QueryPlanner};
use crate::search::query::CandidateStrategy;
use crate::search::request::{
    CandidateContent, SearchCollection, SearchExecution, SearchInput, StreamInput,
};
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

pub trait CandidateContentSource {
    /// # Errors
    ///
    /// Returns an error if transformed content cannot be read for any candidate.
    fn read(&self, candidates: &[Candidate]) -> crate::Result<Vec<CandidateContent>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CandidateOrderKey {
    #[default]
    None,
    Path,
    Modified,
    Accessed,
    Created,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CandidateOrderDirection {
    #[default]
    Ascending,
    Descending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CandidateOrder {
    pub key: CandidateOrderKey,
    pub direction: CandidateOrderDirection,
}

impl CandidateOrder {
    #[must_use]
    pub const fn new(key: CandidateOrderKey, direction: CandidateOrderDirection) -> Self {
        Self { key, direction }
    }

    #[must_use]
    pub const fn is_sorted(self) -> bool {
        !matches!(self.key, CandidateOrderKey::None)
    }

    /// Order candidates in place according to the configured key.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when filesystem metadata required by a timestamp
    /// ordering key cannot be read.
    pub fn order(self, candidates: &mut [Candidate]) -> crate::Result<()> {
        if !self.is_sorted() {
            return Ok(());
        }

        let mut keyed = Vec::with_capacity(candidates.len());
        for candidate in candidates.iter().cloned() {
            keyed.push(CandidateOrderEntry::new(candidate, self.key)?);
        }

        keyed.sort_by(|a, b| {
            a.value
                .cmp(&b.value)
                .then_with(|| a.rel_path.cmp(&b.rel_path))
        });
        if matches!(self.direction, CandidateOrderDirection::Descending) {
            keyed.reverse();
        }

        for (slot, entry) in candidates.iter_mut().zip(keyed) {
            *slot = entry.candidate;
        }

        Ok(())
    }
}

struct CandidateOrderEntry {
    value: CandidateOrderValue,
    rel_path: PathBuf,
    candidate: Candidate,
}

impl CandidateOrderEntry {
    fn new(candidate: Candidate, key: CandidateOrderKey) -> crate::Result<Self> {
        let rel_path = candidate.rel_path().to_path_buf();
        let value = match key {
            CandidateOrderKey::None | CandidateOrderKey::Path => {
                CandidateOrderValue::Path(rel_path.clone())
            }
            CandidateOrderKey::Modified => CandidateOrderValue::Time(candidate_time(
                candidate.abs_path(),
                std::fs::Metadata::modified,
            )?),
            CandidateOrderKey::Accessed => CandidateOrderValue::Time(candidate_time(
                candidate.abs_path(),
                std::fs::Metadata::accessed,
            )?),
            CandidateOrderKey::Created => CandidateOrderValue::Time(candidate_time(
                candidate.abs_path(),
                std::fs::Metadata::created,
            )?),
        };

        Ok(Self {
            value,
            rel_path,
            candidate,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum CandidateOrderValue {
    Path(PathBuf),
    Time(SystemTime),
}

fn candidate_time(
    path: &Path,
    timestamp: impl FnOnce(&std::fs::Metadata) -> std::io::Result<SystemTime>,
) -> crate::Result<SystemTime> {
    Ok(timestamp(&std::fs::metadata(path)?)?)
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
    pub candidate_order: CandidateOrder,
    pub content_source: Option<&'a dyn CandidateContentSource>,
}

impl GrepRequest<'_> {
    /// Search one or more source kinds as a single grep execution.
    ///
    /// # Errors
    ///
    /// Returns an error if candidate resolution, regex compilation, transformed input, or search
    /// execution fails.
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

        let compiled = query.compile()?;

        let candidates = if corpus_requested {
            self.resolve_candidates(query, compiled.candidate_strategy())?
        } else {
            Vec::new()
        };

        let transformed = if corpus_requested {
            self.content_source
                .map(|source| source.read(&candidates))
                .transpose()?
        } else {
            None
        };

        let mut inputs = Vec::with_capacity(
            transformed
                .as_ref()
                .map_or_else(|| usize::from(!candidates.is_empty()), Vec::len)
                + streams.len(),
        );
        if let Some(transformed) = transformed.as_deref() {
            if !transformed.is_empty() {
                inputs.push(SearchInput::Transformed(transformed));
            }
        } else if !candidates.is_empty() {
            inputs.push(SearchInput::Candidates(&candidates));
        }
        inputs.extend(streams.into_iter().map(SearchInput::Stream));

        self.search_inputs(query, inputs)
    }

    fn resolve_candidates(
        &self,
        query: &SearchQuery,
        candidate_strategy: CandidateStrategy,
    ) -> crate::Result<Vec<Candidate>> {
        let spec = query.build_query_spec();
        let output = self.output;
        let requirement = if self.content_source.is_some() {
            CandidateRequirement::Complete
        } else {
            match candidate_strategy {
                CandidateStrategy::Indexed => output.candidate_requirement(),
                CandidateStrategy::Complete(_) => CandidateRequirement::Complete,
            }
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

        let mut candidates: Vec<Candidate> = raw
            .into_par_iter()
            .filter(|c| c.matches(self.filter))
            .collect();
        self.candidate_order.order(&mut candidates)?;
        Ok(candidates)
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
