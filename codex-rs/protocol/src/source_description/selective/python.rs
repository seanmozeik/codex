//! Source-declared Python text policies and conditional logical stream endpoints.
use super::SelectiveTextEncoding;
use super::SelectiveTextRecipe;
use super::SourceDescriptionSpan;
use schemars::JsonSchema;
use schemars::r#gen::SchemaGenerator;
use schemars::schema::Schema;
use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

/// Full-read decoding policy; omission on a recipe retains universal translation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub enum SelectivePythonReadNewline {
    UniversalTranslate,
    Preserve,
}

/// Source policy, distinct from the caller's concrete runtime-default assertion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub enum SelectivePythonWriteNewline {
    RuntimeDefault,
    Preserve,
    Cr,
    CrLf,
}

/// Source-local owned stream identity, not a file descriptor or pathname.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema, TS)]
#[serde(transparent)]
#[ts(export_to = "v2/")]
pub struct SelectivePythonStreamId(usize);
impl SelectivePythonStreamId {
    pub const fn new(value: usize) -> Self {
        Self(value)
    }
    pub const fn get(self) -> usize {
        self.0
    }
}

/// Only explicit-newline truncate/update text streams enter the logical lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub enum SelectivePythonStreamMode {
    TruncateWrite,
    UpdateExisting,
}

/// Zero-position proof; read completion alone does not establish rewind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[ts(tag = "type", export_to = "v2/")]
pub enum SelectivePythonStreamPosition {
    InitialZero,
    RewoundZero {
        #[serde(rename = "readId")]
        read_id: usize,
        #[serde(rename = "readEvidence")]
        read_evidence: SourceDescriptionSpan,
        #[serde(rename = "seekEvidence")]
        seek_evidence: SourceDescriptionSpan,
    },
}

/// Current write acceptance remains conditional, never a completed predecessor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub enum SelectivePythonWriteResultCondition {
    FullCharacterCount,
}

/// One logical stream write; its base epoch names the physical seed only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "v2/")]
pub struct SelectivePythonLogicalWrite {
    pub stream_id: SelectivePythonStreamId,
    pub opened_at: SourceDescriptionSpan,
    pub mode: SelectivePythonStreamMode,
    pub position: SelectivePythonStreamPosition,
    pub base_filesystem_epoch: usize,
    pub stream_revision: usize,
    pub content: SelectiveTextRecipe,
    pub encoding: SelectiveTextEncoding,
    pub newline: SelectivePythonWriteNewline,
    pub required_result: SelectivePythonWriteResultCondition,
}

/// Binding to an owned stream and physical seed, independent of event ordinal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "v2/")]
pub struct SelectiveLogicalStreamBinding {
    pub source_id: usize,
    pub stream_id: SelectivePythonStreamId,
    pub opened_at: SourceDescriptionSpan,
    pub base_filesystem_epoch: usize,
    pub revision: usize,
}

/// Logical bytes are not an assertion of successful physical flush or close.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub enum SelectiveLogicalStreamRealization {
    RequiresSuccessfulFlushOrClose,
}

/// Conditional logical overlay; never usable as a physical replay seed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "v2/")]
pub struct SelectiveLogicalStreamPreviewMetadata {
    pub binding: SelectiveLogicalStreamBinding,
    pub mode: SelectivePythonStreamMode,
    pub position: SelectivePythonStreamPosition,
    pub revision_after: usize,
    /// UTF-8 payload after output newline translation, excluding retained suffix.
    #[schemars(schema_with = "logical_payload_bytes_schema")]
    pub payload_bytes: Vec<u8>,
    /// Unicode characters accepted before output newline translation.
    pub accepted_characters_required: usize,
    pub realization: SelectiveLogicalStreamRealization,
}

// Explicit bounds let schema consumers enforce u8 even without uint8 format support.
fn logical_payload_bytes_schema(generator: &mut SchemaGenerator) -> Schema {
    let mut schema = Vec::<u8>::json_schema(generator).into_object();
    let mut item = u8::json_schema(generator).into_object();
    item.number().minimum = Some(0.0);
    item.number().maximum = Some(255.0);
    schema.array().items = Some(Schema::Object(item).into());
    schema.into()
}
