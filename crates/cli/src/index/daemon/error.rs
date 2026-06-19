use thiserror::Error;

#[derive(Debug, Error)]
pub enum DaemonError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("daemon error: {0}")]
    Message(String),
}

impl DaemonError {
    pub fn message(msg: impl Into<String>) -> Self {
        Self::Message(msg.into())
    }
}

impl From<anyhow::Error> for DaemonError {
    fn from(value: anyhow::Error) -> Self {
        Self::Message(value.to_string())
    }
}

impl From<sift_core::Error> for DaemonError {
    fn from(value: sift_core::Error) -> Self {
        Self::Message(value.to_string())
    }
}
