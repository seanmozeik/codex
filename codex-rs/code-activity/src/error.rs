//! Matchable failures at parser and transport boundaries.

/// Errors that prevent an activity request or stream from being processed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ActivityError {
    /// A pinned grammar is incompatible with the parser runtime.
    #[error("cannot configure source parser")]
    Grammar(#[from] tree_sitter::LanguageError),
    /// The stream exceeded its source byte limit.
    #[error("stream source byte budget exceeded")]
    SourceLimit,
    /// A previous failure aborted this preview.
    #[error("stream preview has been aborted")]
    StreamAborted,
    /// The parser could not produce a syntax tree.
    #[error("source parser did not produce a syntax tree")]
    ParseInterrupted,
    /// The tool result is not valid JSON with the required field types.
    #[error("cannot decode command result")]
    CommandJson(#[from] serde_json::Error),
    /// A yielded session must be resumed before completion.
    #[error("tool session is still running")]
    SessionRunning,
    /// No terminal exit code was supplied by the host.
    #[error("terminal exit code is missing")]
    MissingExitCode,
}
