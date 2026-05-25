use thiserror::Error;

#[derive(Debug, Error)]
pub enum CompileError {
    #[error("regex build error: {0}")]
    RegexBuild(String),
}
