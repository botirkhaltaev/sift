use thiserror::Error;

use crate::grep::filter::error::FilterError;
use crate::grep::output::error::OutputError;
use crate::grep::pattern::error::CompileError;

#[derive(Debug, Error)]
pub enum GrepError {
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

impl From<CompileError> for GrepError {
    fn from(e: CompileError) -> Self {
        match e {
            CompileError::RegexBuild(s) => Self::RegexBuild(s),
        }
    }
}

impl From<FilterError> for GrepError {
    fn from(e: FilterError) -> Self {
        match e {
            FilterError::RegexBuild(s) => Self::RegexBuild(s),
            FilterError::Ignore(e) => Self::Ignore(e),
        }
    }
}

impl From<OutputError> for GrepError {
    fn from(e: OutputError) -> Self {
        match e {
            OutputError::JsonOutputIncompatibleMode => Self::JsonOutputIncompatibleMode,
            OutputError::JsonSerialize(e) => Self::JsonSerialize(e),
            OutputError::Io(e) => Self::Io(e),
        }
    }
}
