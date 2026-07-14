use std::time::Instant;

use rayon::prelude::*;

use crate::Error;
use crate::GrepError;
use crate::candidates::{Candidates, IndexedCandidates};
use crate::corpus::Candidate;
use crate::search::PrefilterCompatibility;
use crate::search::event::SearchSink;
use crate::search::input::{Input, InputConversion, Inputs, SearchFile, SearchInputs};
use crate::search::matcher::{Matcher, MatcherBuilder};
use crate::search::mode::SearchMode;
use crate::search::options::{SearchBound, SearchOptions};
use crate::search::query::SearchQuery;
use crate::search::report::{Report, SearchSummary};
use crate::search::stats::StatsMode;
use crate::search::task::{SearchOutcome, SearchTask};

#[derive(Debug, Clone)]
pub struct Searcher {
    pub(crate) query: SearchQuery,
    matcher: Matcher,
}

pub enum EventEmission<'a> {
    Discard,
    Emit(&'a mut dyn SearchSink),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::search) enum EventCollection {
    Discard,
    Collect,
}

impl Searcher {
    /// Build a ready searcher by creating the query matcher.
    ///
    /// # Errors
    ///
    /// Returns an error if matcher construction fails.
    pub fn new(query: SearchQuery) -> Result<Self, GrepError> {
        let matcher = MatcherBuilder::new(&query).build()?;
        Ok(Self { query, matcher })
    }
}

impl Searcher {
    #[must_use]
    pub fn patterns(&self) -> &[String] {
        &self.query.patterns
    }

    #[must_use]
    pub const fn options(&self) -> &SearchOptions {
        &self.query.options
    }

    /// Search the given inputs and return a report.
    ///
    /// # Errors
    ///
    /// Returns an error if search execution fails.
    pub fn search(&self, inputs: SearchInputs<'_>, stats: StatsMode) -> crate::Result<Report> {
        self.execute(inputs, stats, SearchMode::Lines, EventEmission::Discard)
    }

    /// Search inputs and emit semantic events to a sink.
    ///
    /// # Errors
    ///
    /// Returns an error if search execution or sink handling fails.
    pub fn stream(
        &self,
        inputs: SearchInputs<'_>,
        mode: SearchMode,
        stats: StatsMode,
        sink: &mut impl SearchSink,
    ) -> crate::Result<Report> {
        self.execute(inputs, stats, mode, EventEmission::Emit(sink))
    }

    pub(crate) fn execute(
        &self,
        inputs: SearchInputs<'_>,
        stats: StatsMode,
        mode: SearchMode,
        events: EventEmission<'_>,
    ) -> crate::Result<Report> {
        if self.options().max_results == Some(0) {
            return Err(Error::Search(GrepError::InvalidMaxCount));
        }
        if inputs.is_empty() {
            return Ok(Report::empty(stats));
        }

        let search_start = Instant::now();
        let event_collection = events.collection();
        let options = self.options();
        let (mut outcomes, inputs_searched, bytes_searched) = match options.search_bound {
            SearchBound::Exhaustive => self.search_exhaustive(inputs, mode, event_collection)?,
            SearchBound::FirstMatch => self.search_first_match(inputs, mode, event_collection)?,
        };
        let summary = SearchSummary {
            mode,
            stats,
            inputs_len: inputs_searched,
            bytes_searched,
            elapsed: search_start.elapsed(),
        };
        events.emit(&mut outcomes)?;
        Ok(Report::from_outcomes(outcomes, summary))
    }

    fn search_exhaustive(
        &self,
        inputs: SearchInputs<'_>,
        mode: SearchMode,
        event_collection: EventCollection,
    ) -> crate::Result<(Vec<SearchOutcome>, usize, u64)> {
        let SearchInputs {
            candidates,
            streams,
            conversion,
        } = inputs;

        let (mut outcomes, mut searched, mut bytes) =
            self.search_candidates(candidates, &conversion, mode, event_collection)?;

        let stream_outcomes = self.search_inputs(streams.as_slice(), mode, event_collection);
        searched += streams.len();
        bytes = bytes.saturating_add(streams.byte_count());
        outcomes.extend(stream_outcomes);

        Ok((outcomes, searched, bytes))
    }

    fn search_candidates(
        &self,
        candidates: Candidates<'_>,
        conversion: &InputConversion<'_>,
        mode: SearchMode,
        event_collection: EventCollection,
    ) -> crate::Result<(Vec<SearchOutcome>, usize, u64)> {
        match candidates {
            Candidates::Resolved(items) => {
                self.search_resolved(&items, conversion, mode, event_collection)
            }
            Candidates::Indexed(indexed) => {
                self.search_indexed(&indexed, conversion, mode, event_collection)
            }
            Candidates::Mixed { indexed, unindexed } => {
                let (mut indexed_outcomes, indexed_searched, indexed_bytes) =
                    self.search_indexed(&indexed, conversion, mode, event_collection)?;
                let (resolved_outcomes, resolved_searched, resolved_bytes) =
                    self.search_resolved(&unindexed, conversion, mode, event_collection)?;
                indexed_outcomes.extend(resolved_outcomes);
                Ok((
                    indexed_outcomes,
                    indexed_searched.saturating_add(resolved_searched),
                    indexed_bytes.saturating_add(resolved_bytes),
                ))
            }
        }
    }

    fn search_resolved(
        &self,
        candidates: &[Candidate],
        conversion: &InputConversion<'_>,
        mode: SearchMode,
        event_collection: EventCollection,
    ) -> crate::Result<(Vec<SearchOutcome>, usize, u64)> {
        let mut corpus_inputs = Inputs::with_capacity(candidates.len());
        for candidate in candidates {
            match conversion.materialize(candidate)? {
                Input::Path {
                    path,
                    identity,
                    explicit,
                } => corpus_inputs.push_path(path, identity, explicit),
                Input::Bytes {
                    path,
                    bytes,
                    identity,
                    explicit,
                } => {
                    if explicit {
                        corpus_inputs.push_explicit_bytes(path, bytes, identity);
                    } else {
                        corpus_inputs.push_bytes(path, bytes, identity);
                    }
                }
            }
        }
        let outcomes = self.search_inputs(corpus_inputs.as_slice(), mode, event_collection);
        let len = corpus_inputs.len();
        let bytes = corpus_inputs.byte_count();
        Ok((outcomes, len, bytes))
    }

    fn search_indexed(
        &self,
        indexed: &IndexedCandidates<'_>,
        conversion: &InputConversion<'_>,
        mode: SearchMode,
        event_collection: EventCollection,
    ) -> crate::Result<(Vec<SearchOutcome>, usize, u64)> {
        let options = self.options();
        let outcomes: crate::Result<Vec<SearchOutcome>> = indexed
            .file_ids()
            .par_iter()
            .filter_map(|&id| {
                let candidate =
                    indexed
                        .indexes
                        .hydrate_row(id, indexed.filter, indexed.admission)?;
                Some(
                    conversion
                        .open(SearchFile::Hydrated(candidate))
                        .map(|input| {
                            let mut grep = SearchTask::discovered_searcher(options, mode);
                            SearchTask::new(&self.matcher, options, mode, event_collection, &input)
                                .execute(&mut grep)
                        }),
                )
            })
            .collect();
        let outcomes = outcomes?;
        let searched = outcomes.len();
        let bytes = outcomes.iter().fold(0u64, |acc, outcome| {
            acc.saturating_add(outcome.bytes_searched)
        });
        Ok((outcomes, searched, bytes))
    }

    fn search_inputs(
        &self,
        inputs: &[Input<'_>],
        mode: SearchMode,
        event_collection: EventCollection,
    ) -> Vec<SearchOutcome> {
        let options = self.options();
        inputs
            .par_iter()
            .map_init(
                || SearchTask::discovered_searcher(options, mode),
                |grep, input| {
                    SearchTask::new(&self.matcher, options, mode, event_collection, input)
                        .execute(grep)
                },
            )
            .collect()
    }

    fn search_first_match(
        &self,
        inputs: SearchInputs<'_>,
        mode: SearchMode,
        event_collection: EventCollection,
    ) -> crate::Result<(Vec<SearchOutcome>, usize, u64)> {
        let options = self.options();
        let SearchInputs {
            candidates,
            streams,
            conversion,
        } = inputs;
        let mut found = Vec::new();
        let mut searched = 0usize;
        let mut bytes = 0u64;
        let mut grep = SearchTask::discovered_searcher(options, mode);

        for candidate in candidates {
            searched += 1;
            let input = conversion.materialize(&candidate)?;
            let outcome = SearchTask::new(&self.matcher, options, mode, event_collection, &input)
                .execute(&mut grep);
            bytes = bytes.saturating_add(outcome.bytes_searched);
            if mode.selects(outcome.matched) {
                found.push(outcome);
                return Ok((found, searched, bytes));
            }
        }
        for input in streams.as_slice() {
            searched += 1;
            let outcome = SearchTask::new(&self.matcher, options, mode, event_collection, input)
                .execute(&mut grep);
            bytes = bytes.saturating_add(outcome.bytes_searched);
            if mode.selects(outcome.matched) {
                found.push(outcome);
                break;
            }
        }

        Ok((found, searched, bytes))
    }

    pub(crate) const fn prefilter_compatibility(&self) -> PrefilterCompatibility {
        self.matcher.prefilter_compatibility()
    }
}

impl EventEmission<'_> {
    const fn collection(&self) -> EventCollection {
        match self {
            Self::Discard => EventCollection::Discard,
            Self::Emit(_) => EventCollection::Collect,
        }
    }

    fn emit(self, outcomes: &mut [SearchOutcome]) -> crate::Result<()> {
        let Self::Emit(sink) = self else {
            return Ok(());
        };
        for event in outcomes.iter_mut().flat_map(SearchOutcome::drain_events) {
            sink.event(event)?;
        }
        Ok(())
    }
}
