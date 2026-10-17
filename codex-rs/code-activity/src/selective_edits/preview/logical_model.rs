//! Caller assertions and typed conditions for owned logical stream endpoints.

use super::SnapshotEpoch;
use super::SnapshotPreviewInput;
use crate::SourceId;
use crate::Span;
use crate::selective_edits::PythonStreamId;
use crate::selective_edits::PythonStreamMode;
use crate::selective_edits::PythonStreamPosition;
use serde::Serialize;

/// Exact owned open generation and revision asserted by the caller.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct LogicalStreamBinding {
    /// Call-local source identity.
    pub source_id: SourceId,
    /// Source-local owned open generation.
    pub stream_id: PythonStreamId,
    /// Exact open-call evidence.
    pub opened_at: Span,
    /// Physical seed identity, separate from operation and stream revisions.
    pub base_filesystem_epoch: SnapshotEpoch,
    /// Logical revision immediately before the selected acceptance.
    pub revision: usize,
}

impl LogicalStreamBinding {
    /// Records unverified caller assertions; materialization checks every field.
    #[must_use]
    pub const fn new(
        source_id: SourceId,
        stream_id: PythonStreamId,
        opened_at: Span,
        base_filesystem_epoch: SnapshotEpoch,
        revision: usize,
    ) -> Self {
        Self {
            source_id,
            stream_id,
            opened_at,
            base_filesystem_epoch,
            revision,
        }
    }
}

/// Separate input prevents silently treating logical bytes as physical mutation.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct LogicalStreamPreviewInput {
    /// Trusted physical seed and selected recipe read snapshots.
    pub snapshot: SnapshotPreviewInput,
    /// Exact stream generation and logical revision.
    pub binding: LogicalStreamBinding,
}

impl LogicalStreamPreviewInput {
    /// Combines explicit stream identity with supplied snapshots without I/O.
    #[must_use]
    pub const fn new(snapshot: SnapshotPreviewInput, binding: LogicalStreamBinding) -> Self {
        Self { snapshot, binding }
    }
}

/// Additional condition required to realize accepted stream content physically.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum LogicalStreamRealization {
    /// Full character acceptance alone establishes no physical visibility.
    RequiresSuccessfulFlushOrClose,
}

/// Logical payload, acceptance condition and identity retained alongside a diff.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct LogicalStreamPreviewMetadata {
    /// Caller assertion checked against source evidence.
    pub binding: LogicalStreamBinding,
    /// Owned open mode.
    pub mode: PythonStreamMode,
    /// Source proof that the stream is at zero.
    pub position: PythonStreamPosition,
    /// Logical revision after the conditional full acceptance.
    pub revision_after: usize,
    /// Translated UTF-8 payload, distinct from the retained seed suffix.
    pub payload_bytes: Vec<u8>,
    /// Required write result in pre-translation Unicode characters.
    pub accepted_characters_required: usize,
    /// Physical visibility is still conditional on later successful I/O.
    pub realization: LogicalStreamRealization,
}
