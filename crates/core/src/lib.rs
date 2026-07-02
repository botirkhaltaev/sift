//! Composable indexed code search engine.
//!
//! `sift-core` builds on-disk indexes over codebases and uses them to narrow
//! candidate files before running the full regex engine.

pub(crate) mod corpus;
pub mod grep;
pub mod index;
pub(crate) mod query;

pub use corpus::Candidate;
pub use corpus::CandidateCoverage;
pub use grep::{
    CandidateFilter, CandidateFilterConfig, CandidateOrder, CandidatePolicy, CandidateScope,
    CompiledQuery, Error as GrepError, GlobConfig, HiddenMode, IgnoreConfig, IgnoreSources, Input,
    Inputs, Match, MatchOptions, Query, Report, Session, Stats, StatsMode, TypeFilterRule,
    VisibilityConfig,
};

pub use ignore::{Walk, WalkBuilder};

pub use index::config::{IndexBuildConfig, IndexWalkConfig};
pub use index::meta::StoreMeta;
pub use index::ngram::{
    Config as NGramConfig, Gram, GramWidth, GramWindows, Index as NGramIndex, NGramIndexError,
};
pub use index::store::IndexStore;
pub use index::{
    CorpusKind, CorpusMeta, CorpusSpec, FileId, FilterMeta, Index, IndexConfig, IndexCoverage,
    IndexError, IndexId, Indexes, PlanMode, QueryPlanOutput, ReconcileOutcome, SnapshotId,
    WalkMeta,
};
pub use query::{QueryFlags, QuerySpec};

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
