//! Grep pipeline orchestration.
//!
//! Bridges the query planner, index registry, candidate filter, and search
//! engine into grep search operations. The pipeline is
//! index-agnostic: it works with whatever index types the `Indexes` registry
//! has opened.

pub mod candidates;
pub mod corpus;
pub mod error;
pub mod filter;
pub mod input;
pub mod options;
pub mod output;
pub mod pattern;
pub mod query;
pub mod report;
pub(crate) mod runner;
pub(crate) mod sink;
mod stats;

use candidates::{CandidateResolver, CandidateSet};
use input::GrepInputs;
use runner::GrepRunner;

pub use crate::walk::{LinkTraversal, WalkOptions};
pub use candidates::{CandidateOrder, CandidateOrderDirection, CandidateOrderKey};
pub use corpus::{CandidateContentSource, CandidateIndexState, GrepCorpus};
pub use error::GrepError;
pub use filter::{
    CandidateFilter, CandidateFilterConfig, GlobConfig, HiddenMode, IgnoreConfig, IgnoreSources,
    TypeFilterRule, VisibilityConfig,
};
pub use input::{CandidateContent, GrepStream};
pub use options::{
    BinaryMode, CaseMode, GrepMatchFlags, GrepOptions, InputEncoding, RegexEngineRequest,
};
pub use output::format::{ColumnLimit, ColumnOverflow};
pub use output::mode::{GrepMode, MatchEmissionMode, OutputEmission, ZeroCountMode};
pub use output::passthru::PassthruMode;
pub use output::style::GrepSeparators;
pub use output::style::{
    ColorChoice, FilenameMode, GrepLineStyle, GrepRecordStyle, LineStyleFlags, PathDisplay,
    RecordTerminator,
};
pub use output::{GrepOutput, GrepOutputFormat};
pub use pattern::PatternCompiler;
pub use query::{GrepQuery, Match};
pub use report::{GrepCollection, GrepOutcome, GrepReport};
pub use stats::GrepStats;

/// High-level grep execution entrypoint.
pub struct Grep<'a> {
    query: GrepQuery,
    corpus: Option<GrepCorpus<'a>>,
    streams: &'a [GrepStream<'a>],
    output: GrepOutput,
    separators: GrepSeparators,
    collect: GrepCollection,
}

impl<'a> Grep<'a> {
    #[must_use]
    pub fn new(query: GrepQuery) -> Self {
        Self {
            query,
            corpus: None,
            streams: &[],
            output: GrepOutput::default(),
            separators: GrepSeparators::default(),
            collect: GrepCollection::default(),
        }
    }

    #[must_use]
    pub const fn corpus(mut self, corpus: GrepCorpus<'a>) -> Self {
        self.corpus = Some(corpus);
        self
    }

    #[must_use]
    pub const fn streams(mut self, streams: &'a [GrepStream<'a>]) -> Self {
        self.streams = streams;
        self
    }

    #[must_use]
    pub fn output(mut self, output: GrepOutput) -> Self {
        self.output = output;
        self
    }

    #[must_use]
    pub fn separators(mut self, separators: &GrepSeparators) -> Self {
        self.separators = separators.clone();
        self
    }

    #[must_use]
    pub const fn collect(mut self, collect: GrepCollection) -> Self {
        self.collect = collect;
        self
    }

    /// Runs grep over the configured corpus and/or byte streams.
    ///
    /// # Errors
    ///
    /// Returns an error if candidate resolution, regex compilation, transformed input, or search
    /// execution fails.
    pub fn run(self) -> crate::Result<GrepReport> {
        let search = &self.query;
        Self::validate_query(search)?;

        let candidates = if let Some(corpus) = self.corpus.as_ref() {
            let compiled = search.compile()?;
            CandidateResolver::new(
                search,
                corpus,
                self.output.clone(),
                compiled.candidate_strategy(),
            )
            .resolve()?
        } else {
            CandidateSet::new(Vec::new(), candidates::CandidateCoverage::PotentialMatches)
        };

        let transformed = if let Some(corpus) = self.corpus.as_ref() {
            let _coverage = candidates.coverage();
            corpus
                .content_source
                .map(|source| source.read(candidates.as_slice()))
                .transpose()?
        } else {
            None
        };

        let mut inputs;
        if let Some(transformed) = transformed.as_deref() {
            inputs = GrepInputs::from_transformed(transformed);
        } else if !candidates.as_slice().is_empty() {
            inputs = GrepInputs::from_candidates(candidates.as_slice());
        } else {
            inputs = GrepInputs::empty();
        }
        inputs.push_streams(self.streams);

        let compiled = search.compile()?;
        GrepRunner::new(
            search,
            compiled,
            self.output,
            &self.separators,
            self.collect.with_hits(true),
        )
        .run(&inputs)
    }

    fn validate_query(query: &GrepQuery) -> crate::Result<()> {
        if query.opts().max_results == Some(0) {
            return Err(crate::Error::Search(GrepError::InvalidMaxCount));
        }

        Ok(())
    }
}
