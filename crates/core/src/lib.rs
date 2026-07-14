//! Composable indexed code search engine.
//!
//! `sift-core` builds on-disk indexes over codebases and uses them to narrow
//! candidate files before running the full regex engine.

pub mod candidates;
pub(crate) mod corpus;
pub mod grep;
pub mod index;
pub mod search;

pub use candidates::{
    CandidateSource, Candidates, IndexNarrowing, IndexedCandidates, ScanScope, SnapshotFreshness,
};
pub use corpus::Candidate;
pub use grep::{
    ByteInput, CandidateFilter, CandidateFilterConfig, CandidateOrder, CandidateTransform,
    Error as GrepError, FilterAdmission, GlobConfig, Grep, GrepRequest, HiddenMode, IgnoreConfig,
    IgnoreSources, TypeFilterRule, VisibilityConfig,
};
pub use search::{
    BinaryEvent, BinaryMode, CaseMode, ContextEvent, ContextKind, FileEvent, FileReport, Input,
    InputConversion, InputEncoding, InputIdentity, Inputs, Match, MatchEvent, RegexEngine, Report,
    SearchBound, SearchEvent, SearchFlags, SearchInputs, SearchMode, SearchOptions, SearchQuery,
    SearchQueryBuilder, SearchSink, Searcher, Stats, StatsMode, ZeroCounts,
};

pub use ignore::{Walk, WalkBuilder};

pub use index::config::{IndexBuildConfig, IndexWalkConfig};
pub use index::meta::StoreMeta;
pub use index::ngram::{
    Config as NGramConfig, Gram, GramWidth, GramWindows, Index as NGramIndex, NGramIndexError,
};
pub use index::store::IndexStore;
pub use index::{
    CorpusKind, CorpusMeta, CorpusSpec, FileId, FilterMeta, IndexConfig, IndexCoverage, IndexError,
    IndexId, IndexedCorpus, Indexes, PlanMode, QueryPlanOutput, SnapshotId, WalkMeta,
};

use thiserror::Error;

pub const SIFT_DIR: &str = ".sift";
pub const FILES_BIN: &str = "files.bin";
pub const LEXICON_BIN: &str = "lexicon.bin";
pub const POSTINGS_BIN: &str = "postings.bin";
pub const GRAMS_BIN: &str = "grams.bin";

/// Top-level umbrella error for all core operations.
#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Index(#[from] IndexError),

    #[error(transparent)]
    Search(#[from] grep::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("ignore walk error: {0}")]
    Ignore(#[from] ignore::Error),
}

impl From<crate::index::ngram::NGramIndexError> for Error {
    fn from(e: crate::index::ngram::NGramIndexError) -> Self {
        Self::Index(IndexError::NGram(e))
    }
}

pub type Result<T> = std::result::Result<T, Error>;
