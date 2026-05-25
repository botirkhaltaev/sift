use thiserror::Error;

#[derive(Debug, Error)]
pub enum ExecutionError {
    #[error("invalid max-count: 0 matches requested")]
    InvalidMaxCount,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("ignore walk error: {0}")]
    Ignore(#[from] ignore::Error),
}
