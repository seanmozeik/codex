//! Bounded source syntax and conditional forecasts. Never execution authority.
//!
//! Consumers must use `into_presentable` before displaying untrusted persisted or
//! wire data. A rejected description loses every child, including submitted patches.

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

mod patch_validation;
mod selective;
mod validation;

pub use selective::SelectiveBarrier;
pub use selective::SelectiveBarrierReason;
pub use selective::SelectiveCarrierAssumption;
pub use selective::SelectiveCarrierBasis;
pub use selective::SelectiveEditBasis;
pub use selective::SelectiveEditDetails;
pub use selective::SelectiveIndexRecipe;
pub use selective::SelectiveJavaScriptAssumption;
pub use selective::SelectiveJavaScriptBarrierReason;
pub use selective::SelectiveJavaScriptDependency;
pub use selective::SelectiveJavaScriptDescriptorPosition;
pub use selective::SelectiveJavaScriptEdit;
pub use selective::SelectiveJavaScriptTextRecipe;
pub use selective::SelectiveJavaScriptWriteEffect;
pub use selective::SelectiveLaterContext;
pub use selective::SelectiveLogicalStreamBinding;
pub use selective::SelectiveLogicalStreamPreviewMetadata;
pub use selective::SelectiveLogicalStreamRealization;
pub use selective::SelectivePredicateRecipe;
pub use selective::SelectivePredictedEdit;
pub use selective::SelectivePythonAssumption;
pub use selective::SelectivePythonBarrierReason;
pub use selective::SelectivePythonDependency;
pub use selective::SelectivePythonEdit;
pub use selective::SelectivePythonFinalState;
pub use selective::SelectivePythonIterationContext;
pub use selective::SelectivePythonLogicalWrite;
pub use selective::SelectivePythonOperation;
pub use selective::SelectivePythonReadNewline;
pub use selective::SelectivePythonStreamId;
pub use selective::SelectivePythonStreamMode;
pub use selective::SelectivePythonStreamPosition;
pub use selective::SelectivePythonWriteNewline;
pub use selective::SelectivePythonWriteResultCondition;
pub use selective::SelectiveSnapshotBasis;
pub use selective::SelectiveSnapshotEncoding;
pub use selective::SelectiveSnapshotPreview;
pub use selective::SelectiveSnapshotReadBinding;
pub use selective::SelectiveSnapshotReadNewline;
pub use selective::SelectiveSnapshotScope;
pub use selective::SelectiveSnapshotTextProfile;
pub use selective::SelectiveSnapshotWriteNewline;
pub use selective::SelectiveTarget;
pub use selective::SelectiveTextEncoding;
pub use selective::SelectiveTextRecipe;

pub const MAX_SOURCE_BYTES: usize = 32_768;
pub const MAX_PATCH_BYTES: usize = 16_320;
pub const MAX_SOURCES: usize = 32;
pub const MAX_REFERENCES: usize = 256;
pub const MAX_PATCHES: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "v2/")]
pub struct SourceDescriptionItem {
    pub id: String,
    pub call_id: String,
    pub description: SourceDescription,
}

/// Completion means whole-source analysis finished, never that a tool ran.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "disposition", rename_all = "camelCase", deny_unknown_fields)]
#[ts(tag = "disposition")]
#[ts(export_to = "v2/")]
pub enum SourceDescription {
    CompleteSyntax {
        sources: Vec<SourceDescriptionSource>,
        references: Vec<SourceDescriptionReference>,
        patches: Vec<SubmittedPatch>,
    },
    /// Conditional forecasts may coexist with opaque regions. No execution claim.
    SelectiveFindings {
        sources: Vec<SourceDescriptionSource>,
        edits: Vec<SelectivePredictedEdit>,
        barriers: Vec<SelectiveBarrier>,
    },
    Fallback {
        reason: SourceDescriptionFallback,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub enum SourceDescriptionFallback {
    UnsupportedSource,
    SourceLimit,
    Deadline,
    Busy,
    Unavailable,
    Presentation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub enum SourceDescriptionLanguage {
    TypeScript,
    Shell,
    Python,
}

/// Original decoded text, including any leading exec pragma. No path resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "v2/")]
pub struct SourceDescriptionSource {
    pub id: usize,
    pub language: SourceDescriptionLanguage,
    pub parent: Option<SourceDescriptionSpan>,
    pub text: String,
}

/// UTF-8 byte offsets, one-based lines and one-based byte columns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "v2/")]
pub struct SourceDescriptionSpan {
    pub source_id: usize,
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: usize,
    pub end_line: usize,
    pub start_column: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub enum SourceDescriptionReferenceKind {
    ImportFrom,
    PathConstructorCall,
    OpenCall,
    ReadCall,
    ReplaceCall,
    TransformCall,
    WriteCall,
    InvocationCall,
    CommandOption,
    OutputConsumer,
    ShellConnector,
    ShellRedirection,
    DirectoryOperand,
    EnvironmentAssignment,
    HeredocData,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "v2/")]
pub struct SourceDescriptionReference {
    pub kind: SourceDescriptionReferenceKind,
    pub evidence: SourceDescriptionSpan,
    pub literal: Option<String>,
}

/// Requested patch text has a separate byte-coordinate domain from JavaScript.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "v2/")]
pub struct SubmittedPatch {
    pub id: usize,
    pub invocation: SourceDescriptionSpan,
    pub argument: SourceDescriptionSpan,
    pub payload: String,
    pub files: Vec<SubmittedPatchFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
#[ts(tag = "kind")]
#[ts(export_to = "v2/")]
pub enum SubmittedPatchFile {
    Add {
        path: SubmittedPatchPath,
        lines: Vec<SubmittedPatchLine>,
    },
    Delete {
        path: SubmittedPatchPath,
    },
    Update {
        path: SubmittedPatchPath,
        #[serde(rename = "moveTo")]
        move_to: Option<SubmittedPatchPath>,
        chunks: Vec<SubmittedPatchChunk>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "v2/")]
pub struct SubmittedPatchPath {
    pub spelling: String,
    pub evidence: SubmittedPatchEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "v2/")]
pub struct SubmittedPatchEvidence {
    pub patch_id: usize,
    pub range: SubmittedPatchRange,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
#[ts(export_to = "v2/")]
pub struct SubmittedPatchRange {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "v2/")]
pub struct SubmittedPatchChunk {
    pub marker: Option<SubmittedPatchEvidence>,
    pub lines: Vec<SubmittedPatchLine>,
    pub end_of_file: Option<SubmittedPatchEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "v2/")]
pub struct SubmittedPatchLine {
    pub prefix: SubmittedPatchLinePrefix,
    pub evidence: SubmittedPatchEvidence,
    pub text: SubmittedPatchEvidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub enum SubmittedPatchLinePrefix {
    Context,
    Plus,
    Minus,
}
