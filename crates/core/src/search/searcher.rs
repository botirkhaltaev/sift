use std::time::Instant;

use rayon::prelude::*;

use crate::Error;
use crate::GrepError;
use crate::search::PrefilterCompatibility;
use crate::search::event::SearchSink;
use crate::search::input::SearchInputs;
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
    pub fn search(&self, inputs: SearchInputs<'_, '_>, stats: StatsMode) -> crate::Result<Report> {
        self.execute(inputs, stats, SearchMode::Lines, EventEmission::Discard)
    }

    /// Search inputs and emit semantic events to a sink.
    ///
    /// # Errors
    ///
    /// Returns an error if search execution or sink handling fails.
    pub fn stream(
        &self,
        inputs: SearchInputs<'_, '_>,
        mode: SearchMode,
        stats: StatsMode,
        sink: &mut impl SearchSink,
    ) -> crate::Result<Report> {
        self.execute(inputs, stats, mode, EventEmission::Emit(sink))
    }

    pub(crate) fn execute(
        &self,
        inputs: SearchInputs<'_, '_>,
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
        inputs: SearchInputs<'_, '_>,
        mode: SearchMode,
        event_collection: EventCollection,
    ) -> crate::Result<(Vec<SearchOutcome>, usize, u64)> {
        let options = self.options();
        match inputs {
            SearchInputs::Complete(inputs) => {
                let outcomes: Vec<_> = inputs
                    .as_slice()
                    .par_iter()
                    .map_init(
                        || SearchTask::discovered_searcher(options, mode),
                        |grep, input| {
                            SearchTask::new(&self.matcher, options, mode, event_collection, input)
                                .execute(grep)
                        },
                    )
                    .collect();
                let len = inputs.len();
                let bytes = inputs.byte_count();
                Ok((outcomes, len, bytes))
            }
            SearchInputs::Progressive {
                candidates,
                streams,
                plan,
            } => {
                let mut outcomes = Vec::with_capacity(candidates.len() + streams.len());
                let mut bytes = 0u64;
                let mut grep = SearchTask::discovered_searcher(options, mode);
                for candidate in candidates {
                    let input = plan.materialize(candidate)?;
                    let outcome =
                        SearchTask::new(&self.matcher, options, mode, event_collection, &input)
                            .execute(&mut grep);
                    bytes = bytes.saturating_add(outcome.bytes_searched);
                    outcomes.push(outcome);
                }
                for input in streams.as_slice() {
                    let outcome =
                        SearchTask::new(&self.matcher, options, mode, event_collection, input)
                            .execute(&mut grep);
                    bytes = bytes.saturating_add(outcome.bytes_searched);
                    outcomes.push(outcome);
                }
                let len = candidates.len() + streams.len();
                Ok((outcomes, len, bytes))
            }
        }
    }

    fn search_first_match(
        &self,
        inputs: SearchInputs<'_, '_>,
        mode: SearchMode,
        event_collection: EventCollection,
    ) -> crate::Result<(Vec<SearchOutcome>, usize, u64)> {
        let options = self.options();
        let mut found = Vec::new();
        let mut searched = 0usize;
        let mut bytes = 0u64;
        let mut grep = SearchTask::discovered_searcher(options, mode);

        match inputs {
            SearchInputs::Complete(inputs) => {
                for input in inputs.as_slice() {
                    searched += 1;
                    let outcome =
                        SearchTask::new(&self.matcher, options, mode, event_collection, input)
                            .execute(&mut grep);
                    bytes = bytes.saturating_add(outcome.bytes_searched);
                    if mode.selects(outcome.matched) {
                        found.push(outcome);
                        break;
                    }
                }
            }
            SearchInputs::Progressive {
                candidates,
                streams,
                plan,
            } => {
                for candidate in candidates {
                    searched += 1;
                    let input = plan.materialize(candidate)?;
                    let outcome =
                        SearchTask::new(&self.matcher, options, mode, event_collection, &input)
                            .execute(&mut grep);
                    bytes = bytes.saturating_add(outcome.bytes_searched);
                    if mode.selects(outcome.matched) {
                        found.push(outcome);
                        break;
                    }
                }
                if found.is_empty() {
                    for input in streams.as_slice() {
                        searched += 1;
                        let outcome =
                            SearchTask::new(&self.matcher, options, mode, event_collection, input)
                                .execute(&mut grep);
                        bytes = bytes.saturating_add(outcome.bytes_searched);
                        if mode.selects(outcome.matched) {
                            found.push(outcome);
                            break;
                        }
                    }
                }
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
