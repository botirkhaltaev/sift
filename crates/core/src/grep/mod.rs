//! Grep pipeline orchestration.

pub mod error;
pub mod input;

use crate::candidates::plan::CandidatePlanner;
use crate::candidates::query::CandidateQuery;
use crate::candidates::{CandidateCoverage, CandidateSelection, CandidateSource};
use crate::search::{
    EventEmission, Report, SearchInputs, SearchMode, SearchQuery, SearchSink, Searcher, StatsMode,
    ZeroCounts,
};
pub use crate::search::{InputConversion, Inputs};

pub use crate::corpus::Candidate;
pub use crate::corpus::candidate::PathDisplay;
pub use crate::corpus::filter::{
    CandidateFilter, CandidateFilterConfig, FilterAdmission, GlobConfig, HiddenMode, IgnoreConfig,
    IgnoreSources, TypeFilterRule, VisibilityConfig,
};
pub use crate::corpus::order::{CandidateOrder, CandidateOrderDirection, CandidateOrderKey};
pub use crate::corpus::walk::{AllFiles, FileWalk, WalkFile, WalkMetadata, WalkSelector};
pub use crate::index::Indexes;
pub use crate::search::CandidateTransform;
pub use error::Error;
pub use input::ByteInput;

pub struct Grep<'a> {
    source: CandidateSource<'a>,
}

pub struct GrepBuilder<'grep, 'source, 'input> {
    grep: &'grep Grep<'source>,
    query: Option<SearchQuery>,
    selection: Option<CandidateSelection>,
    streams: Inputs<'input>,
    conversion: InputConversion<'input>,
    mode: SearchMode,
    stats: StatsMode,
}

pub struct GrepRequest<'a> {
    pub query: SearchQuery,
    pub selection: CandidateSelection,
    pub streams: Inputs<'a>,
    pub conversion: InputConversion<'a>,
    pub mode: SearchMode,
    pub stats: StatsMode,
}

impl<'a> Grep<'a> {
    #[must_use]
    pub const fn new(source: CandidateSource<'a>) -> Self {
        Self { source }
    }

    #[must_use]
    pub fn builder<'input>(&self) -> GrepBuilder<'_, 'a, 'input> {
        GrepBuilder {
            grep: self,
            query: None,
            selection: None,
            streams: Inputs::empty(),
            conversion: InputConversion::for_candidates(&[], PathDisplay::Relative, None),
            mode: SearchMode::Lines,
            stats: StatsMode::Off,
        }
    }

    /// Run a grep search and collect the requested report fields.
    ///
    /// # Errors
    ///
    /// Returns an error if query compilation, candidate resolution, or search fails.
    pub fn search(&self, request: GrepRequest<'a>) -> crate::Result<Report> {
        self.execute(request, EventEmission::Discard)
    }

    /// Run a grep search and emit semantic search events to a sink.
    ///
    /// # Errors
    ///
    /// Returns an error if query compilation, candidate resolution, search, or sink handling fails.
    pub fn stream(
        &self,
        request: GrepRequest<'a>,
        sink: &mut impl SearchSink,
    ) -> crate::Result<Report> {
        self.execute(request, EventEmission::Emit(sink))
    }

    /// Resolve corpus candidates for a search request without running search.
    ///
    /// # Errors
    ///
    /// Returns an error if query compilation or candidate resolution fails.
    pub fn resolve_candidates(
        &'a self,
        request: &GrepRequest<'_>,
    ) -> crate::Result<crate::Candidates<'a>> {
        let coverage = request.candidate_coverage();
        let searcher = Searcher::new(request.query.clone())?;
        let candidate_query =
            CandidateQuery::new(&request.query, searcher.prefilter_compatibility());
        let plan =
            CandidatePlanner::plan(&self.source, &candidate_query, request.selection, coverage);
        plan.resolve(&self.source)
    }

    fn execute(
        &self,
        request: GrepRequest<'a>,
        events: EventEmission<'_>,
    ) -> crate::Result<Report> {
        let candidates = self.resolve_candidates(&request)?;
        let searcher = Searcher::new(request.query)?;
        let inputs = SearchInputs {
            candidates,
            streams: request.streams,
            conversion: request.conversion,
        };
        searcher.execute(inputs, request.stats, request.mode, events)
    }
}

impl GrepRequest<'_> {
    const fn candidate_coverage(&self) -> CandidateCoverage {
        match self.mode {
            SearchMode::FilesWithoutMatch => CandidateCoverage::Complete,
            SearchMode::CountLines { zeros } | SearchMode::CountMatches { zeros } => match zeros {
                ZeroCounts::Include => CandidateCoverage::Complete,
                ZeroCounts::Omit => CandidateCoverage::PotentialMatches,
            },
            SearchMode::Lines | SearchMode::Matches | SearchMode::FilesWithMatches => {
                CandidateCoverage::PotentialMatches
            }
        }
    }
}

impl<'input> GrepBuilder<'_, '_, 'input> {
    #[must_use]
    pub fn query(mut self, query: SearchQuery) -> Self {
        self.query = Some(query);
        self
    }

    #[must_use]
    pub const fn selection(mut self, selection: CandidateSelection) -> Self {
        self.selection = Some(selection);
        self
    }

    #[must_use]
    pub fn streams(mut self, streams: Inputs<'input>) -> Self {
        self.streams = streams;
        self
    }

    #[must_use]
    pub const fn conversion(mut self, conversion: InputConversion<'input>) -> Self {
        self.conversion = conversion;
        self
    }

    #[must_use]
    pub const fn mode(mut self, mode: SearchMode) -> Self {
        self.mode = mode;
        self
    }

    #[must_use]
    pub const fn stats(mut self, stats: StatsMode) -> Self {
        self.stats = stats;
        self
    }

    /// Build the canonical grep request.
    ///
    /// # Errors
    ///
    /// Returns an error when required request fields are missing.
    pub fn build(self) -> crate::Result<GrepRequest<'input>> {
        let query = self
            .query
            .ok_or(crate::Error::Search(Error::MissingSearchQuery))?;
        let selection = self
            .selection
            .ok_or(crate::Error::Search(Error::MissingCandidateSelection))?;
        Ok(GrepRequest {
            query,
            selection,
            streams: self.streams,
            conversion: self.conversion,
            mode: self.mode,
            stats: self.stats,
        })
    }

    /// Execute this request and return a report.
    ///
    /// # Errors
    ///
    /// Returns an error if request building, candidate resolution, or search fails.
    pub fn search(self) -> crate::Result<Report> {
        self.grep.search(self.build()?)
    }

    /// Execute this request and emit semantic events.
    ///
    /// # Errors
    ///
    /// Returns an error if request building, candidate resolution, search, or sink handling fails.
    pub fn stream(self, sink: &mut impl SearchSink) -> crate::Result<Report> {
        self.grep.stream(self.build()?, sink)
    }
}

#[cfg(test)]
mod candidate_coverage_tests {
    use super::*;
    use crate::search::SearchQueryBuilder;

    fn coverage(mode: SearchMode) -> CandidateCoverage {
        GrepRequest {
            query: SearchQueryBuilder::new(vec!["x".into()])
                .build()
                .expect("query"),
            selection: CandidateSelection::None,
            streams: Inputs::empty(),
            conversion: InputConversion::for_candidates(&[], PathDisplay::Relative, None),
            mode,
            stats: StatsMode::Off,
        }
        .candidate_coverage()
    }

    #[test]
    fn count_lines_omit_uses_potential_matches() {
        assert_eq!(
            coverage(SearchMode::CountLines {
                zeros: ZeroCounts::Omit
            }),
            CandidateCoverage::PotentialMatches
        );
    }

    #[test]
    fn count_lines_include_uses_complete() {
        assert_eq!(
            coverage(SearchMode::CountLines {
                zeros: ZeroCounts::Include
            }),
            CandidateCoverage::Complete
        );
    }

    #[test]
    fn count_matches_omit_uses_potential_matches() {
        assert_eq!(
            coverage(SearchMode::CountMatches {
                zeros: ZeroCounts::Omit
            }),
            CandidateCoverage::PotentialMatches
        );
    }

    #[test]
    fn files_without_match_uses_complete() {
        assert_eq!(
            coverage(SearchMode::FilesWithoutMatch),
            CandidateCoverage::Complete
        );
    }
}
