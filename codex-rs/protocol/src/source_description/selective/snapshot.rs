//! Conditional endpoint artifacts captured before execution, never observed edits.

use super::SelectiveTarget;
use super::SelectiveTextEncoding;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

pub const MAX_SNAPSHOT_PREVIEWS: usize = 16;
pub const MAX_SNAPSHOT_FILE_BYTES: usize = 65_536;
pub const MAX_SNAPSHOT_DIFF_BYTES: usize = MAX_SNAPSHOT_FILE_BYTES * 2 + 4096;
pub const MAX_SNAPSHOT_PAYLOAD_BYTES: usize = 1_048_576;

/// Captured path identities, existence/absence and bytes must remain applicable
/// until each modeled operation/read, except for the listed replayed effects.
/// Startup/import behavior, ordinary APIs and normal completion remain premises.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub enum SelectiveSnapshotBasis {
    PreExecutionSnapshotConditional,
}

/// The artifact describes one modeled operation, not the script's final state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub enum SelectiveSnapshotScope {
    OperationPoint,
    LogicalStream,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub enum SelectiveSnapshotEncoding {
    Utf8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub enum SelectiveSnapshotReadNewline {
    Unspecified,
    PythonUniversal,
    SourceDeclared,
    Preserve,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub enum SelectiveSnapshotWriteNewline {
    Unspecified,
    Lf,
    Cr,
    CrLf,
    Preserve,
}

/// Explicit text-I/O premises supplied by the selected execution environment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "v2/")]
pub struct SelectiveSnapshotTextProfile {
    pub read_newline: SelectiveSnapshotReadNewline,
    pub write_newline: SelectiveSnapshotWriteNewline,
    /// None preserves an unresolved Python default codec.
    pub runtime_default_encoding: Option<SelectiveSnapshotEncoding>,
}

/// One consumed source read; the binding alone does not attest filesystem truth.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "v2/")]
pub struct SelectiveSnapshotReadBinding {
    pub source_id: usize,
    pub read_id: usize,
    pub filesystem_epoch: usize,
    pub target: SelectiveTarget,
    pub encoding: SelectiveTextEncoding,
    pub newline: SelectiveSnapshotReadNewline,
}

/// A bounded diff derived from captured bytes and modeled preceding operations.
/// Source/API hypotheses and later uncertainty remain on the containing edit.
/// This payload does not establish that execution started, succeeded, or applied
/// any bytes. Display validation does not authenticate a persisted producer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "v2/")]
pub struct SelectiveSnapshotPreview {
    pub basis: SelectiveSnapshotBasis,
    pub scope: SelectiveSnapshotScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub logical_stream: Option<super::SelectiveLogicalStreamPreviewMetadata>,
    pub source_id: usize,
    pub filesystem_epoch: usize,
    /// None means captured absence, never unreadable or unavailable content.
    pub before: Option<String>,
    pub after: String,
    pub unified_diff: String,
    pub read_bindings: Vec<SelectiveSnapshotReadBinding>,
    pub text_profile: SelectiveSnapshotTextProfile,
    /// Capture context is separate from the original source path spelling/cwd.
    pub environment_id: String,
    pub resolved_cwd: String,
    pub resolved_target: String,
    /// Earlier outer operation ordinals replayed to obtain this operation point.
    pub replay_ordinals: Vec<usize>,
}
