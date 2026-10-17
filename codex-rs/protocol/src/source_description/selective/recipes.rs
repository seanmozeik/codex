//! Unevaluated Python text recipes. These contain source values, never snapshots.

use super::SelectiveTarget;
use super::SelectiveTextEncoding;
use super::SourceDescriptionSpan;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

/// Read IDs and epochs are local to evidence.sourceId and frozen evaluation order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[ts(tag = "type")]
#[ts(export_to = "v2/")]
pub enum SelectiveTextRecipe {
    Literal {
        value: String,
        evidence: SourceDescriptionSpan,
    },
    Read {
        #[serde(rename = "readId")]
        read_id: usize,
        target: SelectiveTarget,
        evidence: SourceDescriptionSpan,
        #[serde(rename = "filesystemEpoch")]
        filesystem_epoch: usize,
        encoding: SelectiveTextEncoding,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        newline: Option<super::SelectivePythonReadNewline>,
    },
    Replace {
        input: Box<SelectiveTextRecipe>,
        old: String,
        new: String,
        count: Option<usize>,
        evidence: SourceDescriptionSpan,
    },
    ReplaceText {
        input: Box<SelectiveTextRecipe>,
        old: Box<SelectiveTextRecipe>,
        new: Box<SelectiveTextRecipe>,
        count: Option<usize>,
        evidence: SourceDescriptionSpan,
    },
    /// Python code-point bounds, with an implicit positive unit step.
    Slice {
        input: Box<SelectiveTextRecipe>,
        start: Option<Box<SelectiveIndexRecipe>>,
        end: Option<Box<SelectiveIndexRecipe>>,
        evidence: SourceDescriptionSpan,
    },
    SplitItem {
        input: Box<SelectiveTextRecipe>,
        separator: String,
        maxsplit: Option<i64>,
        index: i64,
        #[serde(rename = "splitEvidence")]
        split_evidence: SourceDescriptionSpan,
        evidence: SourceDescriptionSpan,
    },
    /// Characters are a set of code points, not a suffix.
    Rstrip {
        input: Box<SelectiveTextRecipe>,
        characters: String,
        evidence: SourceDescriptionSpan,
    },
    Concat {
        left: Box<SelectiveTextRecipe>,
        right: Box<SelectiveTextRecipe>,
        evidence: SourceDescriptionSpan,
    },
    /// Only the selected arm is evaluated; neither arm is an observed result.
    Conditional {
        predicate: Box<SelectivePredicateRecipe>,
        consequence: Box<SelectiveTextRecipe>,
        alternative: Box<SelectiveTextRecipe>,
        evidence: SourceDescriptionSpan,
    },
}

/// A frozen Python integer expression, distinct from a JavaScript string index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
#[ts(tag = "type")]
#[ts(export_to = "v2/")]
pub enum SelectiveIndexRecipe {
    Literal {
        value: i64,
        evidence: SourceDescriptionSpan,
    },
    SubstringIndex {
        input: Box<SelectiveTextRecipe>,
        needle: String,
        start: Option<Box<SelectiveIndexRecipe>>,
        evidence: SourceDescriptionSpan,
    },
}

/// Closed, side-effect-free Python predicates over captured text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[ts(tag = "type")]
#[ts(export_to = "v2/")]
pub enum SelectivePredicateRecipe {
    /// Evaluate the immutable text needle before the haystack.
    TextContains {
        needle: Box<SelectiveTextRecipe>,
        haystack: Box<SelectiveTextRecipe>,
        evidence: SourceDescriptionSpan,
    },
    /// Text equality without truth coercion or object dispatch.
    TextEquals {
        left: Box<SelectiveTextRecipe>,
        right: Box<SelectiveTextRecipe>,
        evidence: SourceDescriptionSpan,
    },
    /// Boolean predicates only; evaluate right only if left is true.
    And {
        left: Box<SelectivePredicateRecipe>,
        right: Box<SelectivePredicateRecipe>,
        evidence: SourceDescriptionSpan,
    },
    StartsWith {
        input: Box<SelectiveTextRecipe>,
        prefix: String,
        #[serde(rename = "prefixEvidence")]
        prefix_evidence: SourceDescriptionSpan,
        evidence: SourceDescriptionSpan,
    },
    EndsWith {
        input: Box<SelectiveTextRecipe>,
        suffix: String,
        #[serde(rename = "suffixEvidence")]
        suffix_evidence: SourceDescriptionSpan,
        evidence: SourceDescriptionSpan,
    },
}
