//! Grep pipeline orchestration.

pub mod error;
pub mod input;

use crate::candidates::{
    CandidateExtent, CandidatePlanner, CandidateSelection, CandidateSource, CandidateSpec,
};
use crate::search::{
    EventEmission, PrefilterCompatibility, Report, SearchMode, SearchQuery, SearchSink, Searcher,
    StatsMode, ZeroCounts,
};

pub use crate::corpus::Candidate;
pub use crate::corpus::candidate::PathDisplay;
pub use crate::corpus::filter::{
    CandidateFilter, CandidateFilterConfig, GlobConfig, HiddenMode, IgnoreConfig, IgnoreSources,
    TypeFilterRule, VisibilityConfig,
};
pub use crate::corpus::order::{CandidateOrder, CandidateOrderDirection, CandidateOrderKey};
pub use crate::corpus::walk::{
    CandidateRecords, FileWalk, RelativePaths, WalkFile, WalkProjection,
};
pub use crate::index::Indexes;
pub use error::Error;
pub use input::{ByteInput, CandidateTransform, InputRequest};

pub struct Grep<'a> {
    source: CandidateSource<'a>,
}

pub struct GrepBuilder<'grep, 'source, 'input> {
    grep: &'grep Grep<'source>,
    query: Option<SearchQuery>,
    candidates: Option<CandidateSelection>,
    inputs: InputRequest<'input>,
    mode: SearchMode,
    stats: StatsMode,
}

pub struct GrepRequest<'a> {
    pub query: SearchQuery,
    pub candidates: CandidateSelection,
    pub inputs: InputRequest<'a>,
    pub mode: SearchMode,
    pub stats: StatsMode,
}

impl<'a> Grep<'a> {
    #[must_use]
    pub const fn new(source: CandidateSource<'a>) -> Self {
        Self { source }
    }

    #[must_use]
    pub const fn builder<'input>(&self) -> GrepBuilder<'_, 'a, 'input> {
        GrepBuilder {
            grep: self,
            query: None,
            candidates: None,
            inputs: InputRequest::from_candidates(),
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

    fn execute(
        &self,
        request: GrepRequest<'a>,
        events: EventEmission<'_>,
    ) -> crate::Result<Report> {
        let candidate_extent = request.candidate_extent();
        let query = request.query;
        let searcher = Searcher::new(query.clone())?;
        let mut spec = CandidateSpec::from(&query);
        if matches!(
            searcher.prefilter_compatibility(),
            PrefilterCompatibility::Incompatible
        ) {
            spec.disable_index_narrowing();
        }
        let candidates = CandidatePlanner::new(
            &self.source,
            spec,
            request.candidates.request(candidate_extent),
        )
        .resolve()?;
        let inputs = request.inputs.resolve(&candidates)?;
        searcher.execute(&inputs, request.stats, request.mode, events)
    }
}

impl GrepRequest<'_> {
    const fn candidate_extent(&self) -> CandidateExtent {
        match self.mode {
            SearchMode::CountLines { .. }
            | SearchMode::FilesWithoutMatch
            | SearchMode::CountMatches {
                zeros: ZeroCounts::Include,
            } => CandidateExtent::Complete,
            SearchMode::Lines
            | SearchMode::Matches
            | SearchMode::CountMatches {
                zeros: ZeroCounts::Omit,
            }
            | SearchMode::FilesWithMatches => CandidateExtent::PotentialMatches,
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
    pub const fn candidates(mut self, candidates: CandidateSelection) -> Self {
        self.candidates = Some(candidates);
        self
    }

    #[must_use]
    pub fn inputs(mut self, inputs: InputRequest<'input>) -> Self {
        self.inputs = inputs;
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
        let candidates = self
            .candidates
            .ok_or(crate::Error::Search(Error::MissingCandidateSelection))?;
        Ok(GrepRequest {
            query,
            candidates,
            inputs: self.inputs,
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
