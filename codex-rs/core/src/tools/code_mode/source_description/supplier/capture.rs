//! Entry-time reads use the executor's existing sandbox and stream lifetime.

use super::MAX_CAPTURE_BYTES;
use super::MAX_FILE_BYTES;
use super::MAX_FILES;
use super::SnapshotSupplier;
use codex_code_activity::Target;
use codex_file_system::EnvironmentAccess;
use codex_file_system::GetMetadataOptions;
use codex_utils_path_uri::PathConvention;
use codex_utils_path_uri::PathUri;
use futures::StreamExt;
use std::io::ErrorKind;

pub(super) struct EntrySnapshot {
    target: Target,
    pub(super) cwd: PathUri,
    pub(super) path: PathUri,
    pub(super) bytes: Option<Vec<u8>>,
}

impl SnapshotSupplier {
    pub(super) async fn capture(&mut self, target: &Target) -> Option<&EntrySnapshot> {
        #[cfg(test)]
        self.capture_attempts
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if let Some(index) = self
            .captures
            .iter()
            .position(|value| &value.target == target)
        {
            return self.captures.get(index);
        }
        if self.captures.len() >= MAX_FILES {
            return None;
        }
        let Target::Literal { path, cwd } = target else {
            return None;
        };
        // URI joins normalize dot segments; the OS resolves `..` after any
        // preceding symlink. Such spellings do not share that lexical meaning.
        if path.is_empty()
            || path.ends_with('/')
            || path.starts_with("//")
            || path.rsplit('/').next() == Some(".")
            || path.split('/').any(|component| component == "..")
        {
            return None;
        }
        // POSIX carrier spellings must never be interpreted as host paths for a
        // foreign Windows executor. PathUri retains the selected environment.
        if self.environment.cwd().infer_path_convention() != Some(PathConvention::Posix) {
            return None;
        }
        let cwd = cwd.as_deref().map_or_else(
            || Some(self.environment.cwd().clone()),
            |cwd| self.environment.cwd().join(cwd).ok(),
        )?;
        let requested = cwd.join(path).ok()?;
        let access = self.environment.fs_accessor(None);
        let metadata = access
            .get_metadata(&requested, GetMetadataOptions::default())
            .await;
        let (resolved, bytes) = match metadata {
            Ok(metadata) => {
                if !metadata.is_file || metadata.size > MAX_FILE_BYTES as u64 {
                    return None;
                }
                let resolved = access.canonicalize(&requested).await.ok()?;
                let mut stream = access.read_file_stream(&requested).await.ok()?;
                let mut bytes = Vec::new();
                while let Some(chunk) = stream.next().await {
                    let chunk = chunk.ok()?;
                    let next = bytes.len().checked_add(chunk.len())?;
                    if next > MAX_FILE_BYTES
                        || self.capture_bytes.checked_add(next)? > MAX_CAPTURE_BYTES
                    {
                        return None;
                    }
                    bytes.extend_from_slice(&chunk);
                }
                if bytes.len() as u64 != metadata.size
                    || access
                        .get_metadata(&requested, GetMetadataOptions::default())
                        .await
                        .ok()?
                        != metadata
                    || access.canonicalize(&requested).await.ok()? != resolved
                {
                    return None;
                }
                std::str::from_utf8(&bytes).ok()?;
                (resolved, Some(bytes))
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                // A dangling symlink or an unreadable path is not an absent
                // destination. Only a missing leaf with a readable parent is.
                let parent = requested.parent()?;
                if !access
                    .get_metadata(&parent, GetMetadataOptions::default())
                    .await
                    .ok()?
                    .is_directory
                {
                    return None;
                }
                let parent = access.canonicalize(&parent).await.ok()?;
                let resolved = parent.join(&requested.basename()?).ok()?;
                // No-follow metadata rejects symlinks in every component.
                // Resolve the existing parent through the same sandbox first.
                match access
                    .get_metadata(
                        &resolved,
                        GetMetadataOptions {
                            follow_symlinks: false,
                        },
                    )
                    .await
                {
                    Err(error) if error.kind() == ErrorKind::NotFound => {}
                    _ => return None,
                }
                (resolved, None)
            }
            Err(_) => return None,
        };
        self.capture_bytes = self
            .capture_bytes
            .checked_add(bytes.as_ref().map_or(0, Vec::len))?;
        self.captures.push(EntrySnapshot {
            target: target.clone(),
            cwd,
            path: resolved,
            bytes,
        });
        self.captures.last()
    }
}
