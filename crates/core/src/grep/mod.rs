//! Indexed search execution built on the public grep crates.

mod compile;
mod execution;
mod filter;
mod matcher;
mod options;
mod output;
mod search;

use thiserror::Error;

use compile::error::CompileError;
use execution::error::ExecutionError;
use filter::error::FilterError;
use matcher::error::MatcherError;
use output::error::OutputError;

#[derive(Debug, Error)]
pub enum SearchError {
    #[error("search patterns must not be empty")]
    EmptyPatterns,

    #[error("regex build error: {0}")]
    RegexBuild(String),

    #[error("invalid max-count: 0 matches requested")]
    InvalidMaxCount,

    #[error("JSON output is only supported for standard search (not count or file-list modes)")]
    JsonOutputIncompatibleMode,

    #[error("JSON serialization error: {0}")]
    JsonSerialize(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("ignore walk error: {0}")]
    Ignore(#[from] ignore::Error),
}

impl From<CompileError> for SearchError {
    fn from(e: CompileError) -> Self {
        match e {
            CompileError::RegexBuild(s) => Self::RegexBuild(s),
        }
    }
}

impl From<MatcherError> for SearchError {
    fn from(e: MatcherError) -> Self {
        match e {
            MatcherError::RegexBuild(s) => Self::RegexBuild(s),
        }
    }
}

impl From<FilterError> for SearchError {
    fn from(e: FilterError) -> Self {
        match e {
            FilterError::RegexBuild(s) => Self::RegexBuild(s),
            FilterError::Ignore(e) => Self::Ignore(e),
        }
    }
}

impl From<OutputError> for SearchError {
    fn from(e: OutputError) -> Self {
        match e {
            OutputError::JsonOutputIncompatibleMode => Self::JsonOutputIncompatibleMode,
            OutputError::JsonSerialize(e) => Self::JsonSerialize(e),
            OutputError::Io(e) => Self::Io(e),
        }
    }
}

impl From<ExecutionError> for SearchError {
    fn from(e: ExecutionError) -> Self {
        match e {
            ExecutionError::InvalidMaxCount => Self::InvalidMaxCount,
            ExecutionError::Io(e) => Self::Io(e),
            ExecutionError::Ignore(e) => Self::Ignore(e),
        }
    }
}

pub use compile::PatternCompiler;
pub use execution::config::{LinkTraversal, SearchExecution, WalkOptions};
pub use execution::discover_files;
pub use execution::stats::SearchStats;
pub use filter::{
    CandidateInfo, GlobConfig, HiddenMode, IgnoreConfig, IgnoreSources, SearchFilter,
    SearchFilterConfig, TypeDef, VisibilityConfig,
};
pub use options::{BinaryMode, CaseMode, SearchMatchFlags, SearchOptions};
pub use output::format::{ColumnLimit, ColumnOverflow};
pub use output::mode::{
    CandidateSet, MatchEmissionMode, OutputEmission, SearchMode, ZeroCountMode,
};
pub use output::passthru::PassthruMode;
pub use output::style::{
    ColorChoice, FilenameMode, LineStyleFlags, PathDisplay, RecordTerminator, SearchLineStyle,
    SearchRecordStyle, SearchSeparators,
};
pub use output::{SearchOutput, SearchOutputFormat};
pub use search::{CompiledSearch, Match};
