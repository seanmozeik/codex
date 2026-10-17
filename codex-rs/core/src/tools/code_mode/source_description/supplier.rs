//! Sandboxed entry snapshots and conditional replay for one owned source.
//!
//! Disk bytes belong only to epoch zero. Later operation points are supplied
//! exclusively by the materializer's complete preceding effects, never by
//! relabelling a fresh read. Different target spellings may be hard-link aliases.

mod capture;
mod reads;
mod request;

use crate::session::turn_context::TurnEnvironment;
use codex_code_activity as activity;
use codex_utils_path_uri::PathUri;
use std::time::Instant;
use tokio_util::sync::CancellationToken;

pub(super) const MAX_FILE_BYTES: usize = 65_536;
const MAX_CAPTURE_BYTES: usize = 262_144;
const MAX_FILES: usize = 16;
const MAX_PREVIEWS: usize = 16;

pub(super) struct AutomaticSnapshot {
    pub(super) preview: activity::ConditionalSnapshotPreview,
    pub(super) environment_id: String,
    pub(super) resolved_cwd: PathUri,
    pub(super) resolved_target: PathUri,
    pub(super) replay_ordinals: Vec<usize>,
}

pub(super) struct SnapshotSupplier {
    environment: TurnEnvironment,
    captures: Vec<capture::EntrySnapshot>,
    capture_bytes: usize,
    #[cfg(test)]
    capture_attempts: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl SnapshotSupplier {
    pub(super) fn new(environment: TurnEnvironment) -> Self {
        Self {
            environment,
            captures: Vec::new(),
            capture_bytes: 0,
            #[cfg(test)]
            capture_attempts: std::sync::Arc::default(),
        }
    }

    pub(super) async fn supply(
        self,
        analysis: &activity::SelectiveEditAnalysis,
        cancellation: &CancellationToken,
        deadline: Instant,
    ) -> Vec<AutomaticSnapshot> {
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => Vec::new(),
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => Vec::new(),
            snapshots = self.collect(analysis, cancellation, deadline) => snapshots,
        }
    }

    async fn collect(
        mut self,
        analysis: &activity::SelectiveEditAnalysis,
        cancellation: &CancellationToken,
        deadline: Instant,
    ) -> Vec<AutomaticSnapshot> {
        let mut previews = Vec::new();
        for edit in analysis.edits().iter().take(MAX_PREVIEWS) {
            if cancellation.is_cancelled() || Instant::now() >= deadline {
                break;
            }
            // An earlier shell/tool invocation can change arbitrary paths even
            // when its successful completion is a forecast prerequisite.
            if !edit.predecessor_conditions.is_empty() {
                break;
            }
            let Some((epoch, target, profile)) = request::operation(edit, &self.environment) else {
                break;
            };
            let point = self
                .point(edit.evidence.source_id, epoch, target, &previews)
                .await;
            let Some((bytes, resolved_cwd, resolved_target)) = point else {
                break;
            };
            let before = activity::TrustedTargetSnapshot::new(
                activity::SnapshotTargetBinding::new(
                    edit.evidence.source_id,
                    activity::SnapshotEpoch::new(epoch),
                    target.clone(),
                ),
                bytes,
            );
            let mut input = activity::SnapshotPreviewInput::new(analysis.call_id().clone(), before);
            input.text_profile = profile;
            input.limits.input_bytes = MAX_CAPTURE_BYTES + 32_768;
            input.limits.output_bytes = MAX_FILE_BYTES;
            input.limits.diff_bytes = MAX_FILE_BYTES * 2 + 4096;
            input.limits.work = 4 * 1024 * 1024;
            let mut input = request::BoundInput::new(input, edit);
            let result = loop {
                if cancellation.is_cancelled() || Instant::now() >= deadline {
                    break None;
                }
                match input.materialize(analysis, edit.ordinal) {
                    Ok(preview) => break Some(preview),
                    Err(activity::SnapshotPreviewError::MissingRead { read })
                        if input.snapshot_mut().reads.len() < MAX_FILES =>
                    {
                        let Some(binding) = reads::binding(edit, read) else {
                            break None;
                        };
                        let Some((Some(bytes), _, _)) = self
                            .point(
                                read.source_id,
                                binding.filesystem_epoch.get(),
                                &binding.target,
                                &previews,
                            )
                            .await
                        else {
                            break None;
                        };
                        input
                            .snapshot_mut()
                            .reads
                            .push(activity::TrustedReadSnapshot::new(binding, bytes));
                    }
                    _ => break None,
                }
            };
            let Some(preview) = result else {
                break;
            };
            let replay_ordinals = previews.iter().map(|value| value.preview.ordinal).collect();
            previews.push(AutomaticSnapshot {
                preview,
                environment_id: self.environment.selection.environment_id.clone(),
                resolved_cwd,
                resolved_target,
                replay_ordinals,
            });
            if previews.last().is_some_and(|value| {
                value.preview.scope == activity::SnapshotPreviewScope::LogicalStream
            }) {
                break;
            }
        }
        previews
    }

    async fn point(
        &mut self,
        source_id: activity::SourceId,
        epoch: usize,
        target: &activity::Target,
        previews: &[AutomaticSnapshot],
    ) -> Option<(Option<Vec<u8>>, PathUri, PathUri)> {
        // A logical stream result cannot establish a later physical preimage,
        // even for an entry-epoch read or a differently spelled target.
        if previews
            .iter()
            .any(|value| value.preview.scope != activity::SnapshotPreviewScope::OperationPoint)
        {
            return None;
        }
        if epoch == 0 {
            let snapshot = self.capture(target).await?;
            return Some((
                snapshot.bytes.clone(),
                snapshot.cwd.clone(),
                snapshot.path.clone(),
            ));
        }
        // Exact spelling avoids inventing alias independence (canonical paths
        // alone cannot rule out hard links). Every earlier mutation must have
        // been materialized for this same source and destination.
        let predecessors = previews
            .iter()
            .filter(|value| value.preview.evidence.source_id == source_id)
            .take(epoch)
            .collect::<Vec<_>>();
        if predecessors.len() != epoch
            || predecessors.iter().enumerate().any(|(index, value)| {
                value.preview.filesystem_epoch.get() != index || &value.preview.target != target
            })
        {
            return None;
        }
        let previous = predecessors.last()?;
        Some((
            Some(previous.preview.after_bytes.clone()),
            previous.resolved_cwd.clone(),
            previous.resolved_target.clone(),
        ))
    }
}

#[cfg(test)]
mod tests;
