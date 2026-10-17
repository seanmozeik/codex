//! Conditional Node CommonJS recipes; TypeScript grammar alone is not this premise.

use super::SelectiveTarget;
use super::SourceDescriptionSpan;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

/// One synchronous primitive-string UTF-8 filesystem effect under Node assumptions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "v2/")]
pub struct SelectiveJavaScriptEdit {
    pub ordinal: usize,
    pub evidence: SourceDescriptionSpan,
    pub target: SelectiveTarget,
    pub content: SelectiveJavaScriptTextRecipe,
    // ts-rs also makes defaulted, conditionally omitted fields optional in TS.
    #[serde(
        default,
        skip_serializing_if = "SelectiveJavaScriptWriteEffect::is_overwrite"
    )]
    pub write_effect: SelectiveJavaScriptWriteEffect,
    pub dependencies: Vec<SelectiveJavaScriptDependency>,
    pub predecessor_conditions: Vec<SourceDescriptionSpan>,
}

/// Conditional byte effects of path writes and exclusively owned descriptors.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[ts(tag = "type")]
#[ts(export_to = "v2/")]
pub enum SelectiveJavaScriptWriteEffect {
    #[default]
    Overwrite,
    Truncate {
        #[serde(rename = "descriptorId")]
        descriptor_id: usize,
        #[serde(rename = "openedAt")]
        opened_at: SourceDescriptionSpan,
    },
    /// Append UTF-8 bytes at the operation-time EOF under descriptor premises.
    DescriptorAppend {
        #[serde(rename = "descriptorId")]
        descriptor_id: usize,
        #[serde(rename = "openedAt")]
        opened_at: SourceDescriptionSpan,
    },
    DescriptorWrite {
        #[serde(rename = "descriptorId")]
        descriptor_id: usize,
        #[serde(rename = "openedAt")]
        opened_at: SourceDescriptionSpan,
        #[serde(rename = "byteOffset")]
        byte_offset: usize,
        position: SelectiveJavaScriptDescriptorPosition,
    },
}

impl SelectiveJavaScriptWriteEffect {
    fn is_overwrite(&self) -> bool {
        matches!(self, Self::Overwrite)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub enum SelectiveJavaScriptDescriptorPosition {
    Cursor,
    Absolute,
}

/// Node string semantics are separate from Python recipes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[ts(tag = "type")]
#[ts(export_to = "v2/")]
pub enum SelectiveJavaScriptTextRecipe {
    Literal {
        value: String,
        evidence: SourceDescriptionSpan,
    },
    ReadUtf8 {
        #[serde(rename = "readId")]
        read_id: usize,
        target: SelectiveTarget,
        evidence: SourceDescriptionSpan,
        #[serde(rename = "filesystemEpoch")]
        filesystem_epoch: usize,
    },
    /// String needle, first match only; replacement substitutions are excluded.
    ReplaceFirst {
        input: Box<SelectiveJavaScriptTextRecipe>,
        old: String,
        new: String,
        evidence: SourceDescriptionSpan,
    },
    /// Every nonoverlapping match of a nonempty string; no replacement substitutions.
    ReplaceAll {
        input: Box<SelectiveJavaScriptTextRecipe>,
        old: String,
        new: String,
        evidence: SourceDescriptionSpan,
    },
    /// Nonnegative UTF-16 code-unit bounds; unpaired-surrogate results are refused.
    Slice {
        input: Box<SelectiveJavaScriptTextRecipe>,
        start: usize,
        end: Option<usize>,
        evidence: SourceDescriptionSpan,
    },
    Concat {
        left: Box<SelectiveJavaScriptTextRecipe>,
        right: Box<SelectiveJavaScriptTextRecipe>,
        evidence: SourceDescriptionSpan,
    },
}

/// Captured evaluation dependencies, never authority to execute a request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[ts(tag = "type")]
#[ts(export_to = "v2/")]
pub enum SelectiveJavaScriptDependency {
    Descriptor {
        #[serde(rename = "descriptorId")]
        descriptor_id: usize,
        #[serde(rename = "openedAt")]
        opened_at: SourceDescriptionSpan,
    },
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
        assumption: SelectiveJavaScriptAssumption,
    },
    FilesystemEpoch {
        epoch: usize,
    },
}

/// Conditional standard-runtime hypotheses, never runtime attestations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub enum SelectiveJavaScriptAssumption {
    StandardNodeCommonJsEvaluation,
    CanonicalNodeFs,
    StandardStringMethods,
    NoConcurrentMutation,
    CompleteDescriptorWrites,
    RegularDescriptorFiles,
}

/// The finite reason a Node source region remains unresolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub enum SelectiveJavaScriptBarrierReason {
    UnknownName,
    UnmodeledCall,
    UnsupportedOperation,
}

#[cfg(test)]
mod tests {
    use super::SelectiveJavaScriptEdit;
    use ts_rs::TS;

    #[test]
    fn omitted_overwrite_effect_remains_optional_in_typescript() {
        let declaration = SelectiveJavaScriptEdit::decl();
        assert!(
            declaration.contains("writeEffect?: SelectiveJavaScriptWriteEffect"),
            "{declaration}"
        );
    }
}
