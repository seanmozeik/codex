//! Conditional edit forecasts. These payloads never contain observed file diffs.

mod javascript;
mod python;
mod recipes;
pub(super) mod snapshot;

#[cfg(test)]
mod schema_tests;

pub use javascript::SelectiveJavaScriptAssumption;
pub use javascript::SelectiveJavaScriptBarrierReason;
pub use javascript::SelectiveJavaScriptDependency;
pub use javascript::SelectiveJavaScriptDescriptorPosition;
pub use javascript::SelectiveJavaScriptEdit;
pub use javascript::SelectiveJavaScriptTextRecipe;
pub use javascript::SelectiveJavaScriptWriteEffect;
pub use python::SelectiveLogicalStreamBinding;
pub use python::SelectiveLogicalStreamPreviewMetadata;
pub use python::SelectiveLogicalStreamRealization;
pub use python::SelectivePythonLogicalWrite;
pub use python::SelectivePythonReadNewline;
pub use python::SelectivePythonStreamId;
pub use python::SelectivePythonStreamMode;
pub use python::SelectivePythonStreamPosition;
pub use python::SelectivePythonWriteNewline;
pub use python::SelectivePythonWriteResultCondition;
pub use recipes::SelectiveIndexRecipe;
pub use recipes::SelectivePredicateRecipe;
pub use recipes::SelectiveTextRecipe;
pub use snapshot::SelectiveSnapshotBasis;
pub use snapshot::SelectiveSnapshotEncoding;
pub use snapshot::SelectiveSnapshotPreview;
pub use snapshot::SelectiveSnapshotReadBinding;
pub use snapshot::SelectiveSnapshotReadNewline;
pub use snapshot::SelectiveSnapshotScope;
pub use snapshot::SelectiveSnapshotTextProfile;
pub use snapshot::SelectiveSnapshotWriteNewline;

use super::SourceDescriptionLanguage;
use super::SourceDescriptionSpan;
use super::SubmittedPatch;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

pub const MAX_SELECTIVE_EDITS: usize = 256;
pub const MAX_SELECTIVE_BARRIERS: usize = 512;
pub const MAX_SELECTIVE_NODES: usize = 4096;
pub const MAX_SELECTIVE_RECIPE_DEPTH: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub enum SelectiveEditBasis {
    StandardApiForecast,
    HostToolRequest,
}

/// No variant establishes execution or final filesystem state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub enum SelectiveLaterContext {
    Unobserved,
    OpaqueSuffix,
    KnownLaterTouch,
    PossibleAliasLaterTouch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub enum SelectiveCarrierBasis {
    /// Conditional ordinary lookup/startup behavior, never runtime attestation.
    StandardShellDispatchAndStartup,
    /// The shell-launched interpreter supplies Python 3 CLI semantics.
    StandardPython3Cli,
    /// The exact Node carrier supplies CommonJS command-line semantics.
    StandardNodeCommonJsCli,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "v2/")]
pub struct SelectiveCarrierAssumption {
    pub basis: SelectiveCarrierBasis,
    pub language: SourceDescriptionLanguage,
    pub invocation: SourceDescriptionSpan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "v2/")]
pub struct SelectivePredictedEdit {
    /// Ordered across the analyzed call; not an observed execution index.
    pub ordinal: usize,
    pub basis: SelectiveEditBasis,
    pub evidence: SourceDescriptionSpan,
    pub details: SelectiveEditDetails,
    pub predecessor_conditions: Vec<SourceDescriptionSpan>,
    pub carrier_assumptions: Vec<SelectiveCarrierAssumption>,
    pub later_context: SelectiveLaterContext,
    /// Optional pre-execution snapshot calculation, never an observed edit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub snapshot: Option<SelectiveSnapshotPreview>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
#[ts(tag = "kind")]
#[ts(export_to = "v2/")]
pub enum SelectiveEditDetails {
    Python {
        prediction: SelectivePythonEdit,
        assumptions: Vec<SelectivePythonAssumption>,
    },
    JavaScript {
        prediction: SelectiveJavaScriptEdit,
        assumptions: Vec<SelectiveJavaScriptAssumption>,
    },
    Patch {
        patch: SubmittedPatch,
        cwd: Option<String>,
    },
}

/// A literal spelling and entry anchor; this is not a resolved filesystem identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "v2/")]
pub struct SelectiveTarget {
    pub path: String,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "v2/")]
pub struct SelectivePythonEdit {
    /// Local mutation order, scoped by evidence.sourceId.
    pub ordinal: usize,
    /// Source-literal iteration occurrences, outermost first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub iteration_context: Vec<SelectivePythonIterationContext>,
    pub evidence: SourceDescriptionSpan,
    pub target: SelectiveTarget,
    pub operation: SelectivePythonOperation,
    pub dependencies: Vec<SelectivePythonDependency>,
    pub predecessor_conditions: Vec<SourceDescriptionSpan>,
    pub final_state: SelectivePythonFinalState,
}

/// A bounded literal iteration; repeated source spans are distinct occurrences.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "v2/")]
pub struct SelectivePythonIterationContext {
    pub evidence: SourceDescriptionSpan,
    pub index: usize,
    pub literal_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
#[ts(tag = "type")]
#[ts(export_to = "v2/")]
pub enum SelectivePythonOperation {
    LogicalWrite(SelectivePythonLogicalWrite),
    Truncate,
    WriteText {
        content: SelectiveTextRecipe,
        encoding: SelectiveTextEncoding,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        newline: Option<SelectivePythonWriteNewline>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
#[ts(tag = "type")]
#[ts(export_to = "v2/")]
pub enum SelectivePythonDependency {
    Binding {
        name: String,
        version: usize,
        evidence: SourceDescriptionSpan,
    },
    ReadValue {
        #[serde(rename = "readId")]
        read_id: usize,
        evidence: SourceDescriptionSpan,
    },
    EntryWorkingDirectory {
        cwd: Option<String>,
    },
    Capability {
        assumption: SelectivePythonAssumption,
    },
    FilesystemEpoch {
        epoch: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub enum SelectivePythonAssumption {
    PathlibNewlineSupport,
    StandardPythonEvaluation,
    StandardBuiltinOpen,
    CanonicalPathlib,
    StandardTextMethods,
    NoConcurrentMutation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub enum SelectiveTextEncoding {
    Utf8,
    RuntimeDefault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub enum SelectivePythonFinalState {
    NotEstablished,
    KnownLaterTouch,
    PossibleAliasLaterTouch,
    OpaqueSuccessor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "v2/")]
pub struct SelectiveBarrier {
    pub evidence: SourceDescriptionSpan,
    pub reason: SelectiveBarrierReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
#[ts(tag = "type")]
#[ts(export_to = "v2/")]
pub enum SelectiveBarrierReason {
    Carrier {
        reason: String,
    },
    Python {
        reason: SelectivePythonBarrierReason,
    },
    JavaScript {
        reason: SelectiveJavaScriptBarrierReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub enum SelectivePythonBarrierReason {
    UnknownName,
    UnmodeledCall,
    UnmodeledImport,
    UnsupportedOperation,
    EscapingHandle,
}
