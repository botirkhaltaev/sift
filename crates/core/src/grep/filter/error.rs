use thiserror::Error;

#[derive(Debug, Error)]
pub enum FilterError {
    #[error("regex build error: {0}")]
    RegexBuild(String),

    #[error("ignore walk error: {0}")]
    Ignore(#[from] ignore::Error),
}
