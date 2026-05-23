//! Search-domain errors for pattern compilation, execution, and output.

use thiserror::Error;

/// Errors that can occur during search execution or pattern compilation.
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
