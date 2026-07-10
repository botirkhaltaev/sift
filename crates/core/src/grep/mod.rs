//! Grep pipeline orchestration.

pub mod error;
pub mod input;

use crate::candidates::{
    CandidateExtent, CandidateMaterialization, CandidatePlanner, CandidateSelection,
    CandidateSource, CandidateSpec,
};
use crate::search::{
    EventEmission, InputExtent, PrefilterCompatibility, Report, SearchBound, SearchMode,
    SearchQuery, SearchSink, Searcher, StatsMode, ZeroCounts,
};

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
pub use input::{ByteInput, InputRequest};

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
        let input_extent = request.input_extent();
        let materialization = request.candidate_materialization();
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
        .resolve(materialization)?;
        let inputs = request.inputs.resolve(candidates, input_extent)?;
        searcher.execute(inputs, request.stats, request.mode, events)
    }
}

impl GrepRequest<'_> {
    const fn candidate_extent(&self) -> CandidateExtent {
        match self.mode {
            SearchMode::FilesWithoutMatch => CandidateExtent::Complete,
            SearchMode::CountLines { zeros } | SearchMode::CountMatches { zeros } => match zeros {
                ZeroCounts::Include => CandidateExtent::Complete,
                ZeroCounts::Omit => CandidateExtent::PotentialMatches,
            },
            SearchMode::Lines | SearchMode::Matches | SearchMode::FilesWithMatches => {
                CandidateExtent::PotentialMatches
            }
        }
    }

    const fn input_extent(&self) -> InputExtent {
        match self.query.options().search_bound {
            SearchBound::Exhaustive => InputExtent::Complete,
            SearchBound::FirstMatch => InputExtent::Progressive,
        }
    }

    const fn candidate_materialization(&self) -> CandidateMaterialization {
        match self.input_extent() {
            InputExtent::Complete => CandidateMaterialization::Eager,
            InputExtent::Progressive => CandidateMaterialization::Deferred,
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

#[cfg(test)]
mod candidate_extent_tests {
    use super::*;
    use crate::search::SearchQueryBuilder;

    fn extent(mode: SearchMode) -> CandidateExtent {
        GrepRequest {
            query: SearchQueryBuilder::new(vec!["x".into()])
                .build()
                .expect("query"),
            candidates: CandidateSelection::None,
            inputs: InputRequest::from_candidates(),
            mode,
            stats: StatsMode::Off,
        }
        .candidate_extent()
    }

    #[test]
    fn count_lines_omit_uses_potential_matches() {
        assert_eq!(
            extent(SearchMode::CountLines {
                zeros: ZeroCounts::Omit
            }),
            CandidateExtent::PotentialMatches
        );
    }

    #[test]
    fn count_lines_include_uses_complete() {
        assert_eq!(
            extent(SearchMode::CountLines {
                zeros: ZeroCounts::Include
            }),
            CandidateExtent::Complete
        );
    }

    #[test]
    fn count_matches_omit_uses_potential_matches() {
        assert_eq!(
            extent(SearchMode::CountMatches {
                zeros: ZeroCounts::Omit
            }),
            CandidateExtent::PotentialMatches
        );
    }

    #[test]
    fn files_without_match_uses_complete() {
        assert_eq!(
            extent(SearchMode::FilesWithoutMatch),
            CandidateExtent::Complete
        );
    }
}
