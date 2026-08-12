use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum RipError {
    #[error("the trim range must be finite, start at or after zero, and end after it starts")]
    InvalidTrimRange,
    #[error("only public HTTPS URLs are supported")]
    UnsupportedUrl,
    #[error("the acquisition worker returned an invalid WAV: {0}")]
    Wave(#[from] hound::Error),
    #[error("library database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("file operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("prepared media was not created at {0}")]
    MissingArtifact(PathBuf),
    #[error("prepared media must contain one or two channels")]
    UnsupportedChannelCount,
    #[error("prepared media has an unsupported sample representation")]
    UnsupportedSampleFormat,
    #[error("an internal library lock was poisoned")]
    LibraryUnavailable,
    #[error("{tool} failed: {message}")]
    ExternalTool { tool: &'static str, message: String },
    #[error("the source exceeds the configured {kind} limit")]
    LimitExceeded { kind: &'static str },
    #[error("the worker returned a media path outside its job directory")]
    UnsafeArtifactPath,
    #[error("worker protocol error: {0}")]
    Protocol(String),
}
