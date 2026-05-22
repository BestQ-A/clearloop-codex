use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ClearLoopError {
    #[error("invalid ClearLoop id: {0}")]
    InvalidId(String),

    #[error("failed to read or write {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse or encode JSON at {path}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
}

pub type Result<T> = std::result::Result<T, ClearLoopError>;
