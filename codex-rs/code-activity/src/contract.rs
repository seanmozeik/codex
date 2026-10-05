//! Versioned local consumer contract; independent of Codex app-server protocols.

use crate::Basis;
use crate::CodexCommandActivity;
use crate::CodexSourceActivity;
use crate::Coverage;
use crate::Effect;
use crate::Operation;
use crate::OperationId;
use crate::Report;
use crate::Source;
use crate::Span;
use crate::Target;
use crate::Unresolved;
use crate::UnsupportedCodexCarrier;
use crate::WriteMode;
use serde::Deserialize;
use serde::Serialize;

/// Wire discriminator for the additive local contract.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[non_exhaustive]
pub enum ContractSchema {
    /// Version one. Unknown versions must be rejected by consumers.
    #[serde(rename = "codex.codeActivity.v1")]
    V1,
}

/// Borrowed source evidence, retaining report ordering and local identities.
///
/// No execution status or observed changes are inferred here. The original Codex
/// item remains on [`CodexCommandActivity::item`]. This is an outbound local
/// prototype, not a replacement for a protocol or an inbound trust boundary.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ActivityContract<'a> {
    /// Schema discriminator. Changes to v1 field meanings require a new version.
    pub schema: ContractSchema,
    /// Opaque host call identity; IDs below are scoped to this report revision.
    pub call_id: &'a str,
    /// Supported partial evidence or explicit carrier abstention.
    pub analysis: ContractAnalysis<'a>,
}

/// Supported source evidence and unsupported carriers are distinct states.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
#[non_exhaustive]
pub enum ContractAnalysis<'a> {
    /// Source interpretation; even without gaps it is never exhaustive.
    Supported {
        /// Partial means recognized operations; opaque means none recognized.
        coverage: Coverage,
        /// Source provenance table, including embedded-source parent spans.
        sources: &'a [Source],
        /// Unmodified ordered operations; multiplicity is preserved.
        records: &'a [Operation],
        /// Analysis abstentions; reason text is diagnostic, not a policy code.
        unresolved: &'a [Unresolved],
    },
    /// No report could be produced for this carrier.
    Unsupported {
        /// Explicit carrier or resource-budget abstention.
        reason: UnsupportedCodexCarrier,
    },
}

/// Stable display action; never phrased as a successful runtime event.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum UiAction {
    /// Intent to read contents.
    Read,
    /// Intent to list entries.
    List,
    /// Intent to open a potentially mutating handle.
    Open,
    /// Intent to replace or create contents.
    Write,
    /// Intent to append contents.
    Append,
    /// Intent to transform contents in place.
    Edit,
    /// Intent to remove a target.
    Delete,
    /// Intent to shorten a target.
    Truncate,
    /// Intent to launch a process; its child effects are not established.
    Run,
}

/// Explicit target/argument knowledge for display and escalation.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum ArgumentKnowledge {
    /// Literal arguments were recovered, without filesystem or runtime proof.
    Literal,
    /// At least one target or process argument is unknown.
    Unknown,
}

/// UI view model borrowing the canonical operation instead of copying details.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct UiRow<'a> {
    /// Report-local operation identity; not stable across replacement revisions.
    pub operation_id: OperationId,
    /// Display action inferred from source.
    pub action: UiAction,
    /// Static evidence; never an observed execution receipt.
    pub basis: Basis,
    /// Evidence coordinates in decoded UTF-8 source.
    pub evidence: &'a Span,
    /// Explicit knowledge of target or process arguments.
    pub argument_knowledge: ArgumentKnowledge,
    /// Canonical effect details, retaining write mode, transforms and cwd.
    pub details: &'a Effect,
}

impl<'a> ActivityContract<'a> {
    /// Borrows an existing report without changing its historical JSON format.
    #[must_use]
    pub fn from_report(report: &'a Report) -> Self {
        Self {
            schema: ContractSchema::V1,
            call_id: report.call_id.as_str(),
            analysis: ContractAnalysis::Supported {
                coverage: report.coverage,
                sources: &report.sources,
                records: &report.operations,
                unresolved: &report.unresolved,
            },
        }
    }

    /// Borrows the actual Codex adapter result, preserving carrier abstentions.
    #[must_use]
    pub fn from_codex(activity: &'a CodexCommandActivity<'_>) -> Self {
        match &activity.source_activity {
            CodexSourceActivity::Supported { report, .. } => Self::from_report(report),
            CodexSourceActivity::Unsupported { reason } => Self {
                schema: ContractSchema::V1,
                call_id: &activity.item.id,
                analysis: ContractAnalysis::Unsupported { reason: *reason },
            },
        }
    }

    /// Produces one row per record in original order, without deduplication.
    /// Unsupported carriers yield no rows but remain explicit in `analysis`.
    #[must_use]
    pub fn ui_rows(&self) -> Vec<UiRow<'a>> {
        let ContractAnalysis::Supported { records, .. } = &self.analysis else {
            return Vec::new();
        };
        records.iter().map(UiRow::from_operation).collect()
    }
}

impl<'a> UiRow<'a> {
    fn from_operation(operation: &'a Operation) -> Self {
        let action = match &operation.effect {
            Effect::FileRead { .. } => UiAction::Read,
            Effect::FileList { .. } => UiAction::List,
            Effect::FileOpen { .. } => UiAction::Open,
            Effect::FileWrite {
                mode: WriteMode::Append,
                ..
            } => UiAction::Append,
            Effect::FileWrite { .. } => UiAction::Write,
            Effect::FileEdit { .. } => UiAction::Edit,
            Effect::FileDelete { .. } => UiAction::Delete,
            Effect::FileTruncate { .. } => UiAction::Truncate,
            Effect::ProcessRun { .. } => UiAction::Run,
        };
        Self {
            operation_id: operation.id,
            action,
            basis: operation.basis,
            evidence: &operation.evidence,
            argument_knowledge: argument_knowledge(&operation.effect),
            details: &operation.effect,
        }
    }
}

pub(super) fn argument_knowledge(effect: &Effect) -> ArgumentKnowledge {
    let target = match effect {
        Effect::FileRead { target }
        | Effect::FileList { target }
        | Effect::FileOpen { target, .. }
        | Effect::FileWrite { target, .. }
        | Effect::FileEdit { target, .. }
        | Effect::FileDelete { target }
        | Effect::FileTruncate { target } => target,
        Effect::ProcessRun { argv, shell } => {
            return if argv.is_empty() && shell.is_none() || argv.iter().any(Option::is_none) {
                ArgumentKnowledge::Unknown
            } else {
                ArgumentKnowledge::Literal
            };
        }
    };
    match target {
        Target::Literal { .. } => ArgumentKnowledge::Literal,
        Target::Unresolved => ArgumentKnowledge::Unknown,
    }
}
