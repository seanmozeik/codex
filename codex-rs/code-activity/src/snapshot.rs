//! Bounded, opt-in capture in the execution host filesystem.

use crate::CaptureError;
use fs_err as fs;
use fs_err::OpenOptions;
use serde::Serialize;
use std::io;
use std::io::Read;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

/// Maximum UTF-8 bytes retained for one file.
pub const MAX_FILE_BYTES: usize = 131_072;
/// Total text retained in one capture phase.
pub const MAX_CAPTURE_BYTES: usize = 1_048_576;
/// Maximum unique paths observed for one call.
pub const MAX_CAPTURE_FILES: usize = 256;

/// Missing is distinct from unreadable. Contents stay in host memory, not events.
#[derive(Debug)]
#[non_exhaustive]
pub enum FileState {
    /// Captured original or final UTF-8 contents.
    Text(String),
    /// The path did not exist when checked.
    /// The path did not exist at the captured endpoint.
    Missing,
    /// The path could not be captured; never treat this as absent.
    Unavailable(CaptureError),
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
/// Metadata serialized without original file contents.
#[non_exhaustive]
pub enum SnapshotSummary {
    /// A complete bounded text snapshot is available.
    Text {
        /// Number of captured UTF-8 bytes.
        bytes: usize,
    },
    /// The path did not exist at the captured endpoint.
    Missing,
    /// Original or final contents could not be captured.
    Unavailable {
        /// Human-readable capture failure.
        reason: String,
    },
}

impl FileState {
    /// Returns metadata without disclosing captured text.
    #[must_use]
    pub fn summary(&self) -> SnapshotSummary {
        match self {
            Self::Text(s) => SnapshotSummary::Text { bytes: s.len() },
            Self::Missing => SnapshotSummary::Missing,
            Self::Unavailable(reason) => SnapshotSummary::Unavailable {
                reason: reason.to_string(),
            },
        }
    }
}

/// Implement at the execution host, using that host's filesystem and permissions.
/// A provider must bound its reads; the lifecycle also caps returned content.
pub trait SnapshotProvider {
    /// Captures at most `max_bytes` under the execution host permissions.
    fn capture(&mut self, absolute_path: &Path, max_bytes: usize) -> FileState;
}

/// Opt-in local adapter for text files under one existing directory.
/// This is not a security sandbox or an atomic filesystem snapshot.
#[derive(Debug)]
pub struct LocalFiles {
    root: PathBuf,
}

impl LocalFiles {
    /// Resolves an existing directory as the local observation root.
    ///
    /// # Errors
    /// Returns an error if the root cannot be read or is not a directory.
    pub fn new(root: impl AsRef<Path>) -> Result<Self, CaptureError> {
        let root = fs::canonicalize(root)?;
        if !fs::metadata(&root)?.is_dir() {
            return Err(CaptureError::RootNotDirectory);
        }
        Ok(Self { root })
    }

    fn read(&self, path: &Path, max_bytes: usize) -> Result<Option<String>, CaptureError> {
        let relative = path
            .strip_prefix(&self.root)
            .map_err(|_| CaptureError::OutsideRoot)?;
        if relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
        {
            return Err(CaptureError::NonLocalComponent);
        }
        let mut checked = self.root.clone();
        // Reject symlinks and '..' rather than silently following different targets.
        for component in relative.components() {
            match component {
                Component::Normal(name) => checked.push(name),
                Component::CurDir => continue,
                _ => {
                    return Err(CaptureError::NonLocalComponent);
                }
            }
            match fs::symlink_metadata(&checked) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    return Err(CaptureError::Symlink);
                }
                Ok(_) => (),
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(e.into()),
            }
        }
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use fs_err::os::unix::fs::OpenOptionsExt;
            // Avoid following a replaced leaf symlink or blocking on a FIFO.
            options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let file = options.open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(CaptureError::NotRegular);
        }
        if metadata.len() > max_bytes as u64 {
            return Err(CaptureError::FileLimit);
        }
        let mut bytes = Vec::new();
        file.take(max_bytes as u64 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > max_bytes {
            return Err(CaptureError::FileLimit);
        }
        if bytes.contains(&0) {
            return Err(CaptureError::Binary);
        }
        String::from_utf8(bytes)
            .map(Some)
            .map_err(CaptureError::Encoding)
    }
}

impl SnapshotProvider for LocalFiles {
    fn capture(&mut self, absolute_path: &Path, max_bytes: usize) -> FileState {
        match self.read(absolute_path, max_bytes.min(MAX_FILE_BYTES)) {
            Ok(Some(text)) => FileState::Text(text),
            Ok(None) => FileState::Missing,
            Err(e) => FileState::Unavailable(e),
        }
    }
}

pub fn bounded_capture(
    provider: &mut impl SnapshotProvider,
    path: &Path,
    remaining: &mut usize,
) -> FileState {
    if *remaining == 0 {
        return FileState::Unavailable(CaptureError::TotalLimit);
    }
    let limit = MAX_FILE_BYTES.min(*remaining);
    let state = provider.capture(path, limit);
    match state {
        FileState::Text(s) if s.len() > limit => {
            FileState::Unavailable(CaptureError::ProviderLimit)
        }
        FileState::Text(s) => {
            *remaining -= s.len();
            FileState::Text(s)
        }
        state => state,
    }
}
