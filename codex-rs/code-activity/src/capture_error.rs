//! Typed reasons a file could not be captured.

/// Capture failure distinct from file absence.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CaptureError {
    /// The filesystem operation failed; the error retains its path context.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// The configured root is not a directory.
    #[error("capture root is not a directory")]
    RootNotDirectory,
    /// The path is outside the configured root.
    #[error("path is outside capture root")]
    OutsideRoot,
    /// A path component cannot be accepted by the local adapter.
    #[error("non-local path component")]
    NonLocalComponent,
    /// Following an alias would make the observation ambiguous.
    #[error("symlink observation is unsupported")]
    Symlink,
    /// Directories, devices, sockets and pipes are not text files.
    #[error("not a regular file")]
    NotRegular,
    /// The file exceeds its allowed capture size.
    #[error("file byte budget exceeded")]
    FileLimit,
    /// The phase has exhausted its total text budget.
    #[error("capture byte budget exhausted")]
    TotalLimit,
    /// A provider returned more content than requested.
    #[error("provider exceeded capture byte budget")]
    ProviderLimit,
    /// Content contains a NUL byte.
    #[error("binary content is unsupported")]
    Binary,
    /// Captured bytes cannot be represented as UTF-8 text.
    #[error("non-UTF-8 content is unsupported")]
    Encoding(#[source] std::string::FromUtf8Error),
}
