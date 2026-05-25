use thiserror::Error;

#[derive(Debug, Error)]
pub enum MatcherError {
    #[error("regex build error: {0}")]
    RegexBuild(String),
}
