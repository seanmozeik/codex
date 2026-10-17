//! Snapshot-derived artifacts are separate from observed or applied file diffs.

use super::SnapshotEpoch;
use super::SnapshotReadBinding;
use super::SnapshotTextProfile;
use crate::CallId;
use crate::Span;
use crate::Target;
use crate::selective_edits::JavaScriptAssumption;
use crate::selective_edits::PythonAssumption;
use crate::selective_edits::PythonFinalState;
use crate::selective_edits::PythonIterationContext;
use crate::selective_edits::SelectiveCarrierAssumption;
use crate::selective_edits::SelectiveLaterContext;
use serde::Serialize;

/// Evidence class for bytes computed from trusted, externally supplied inputs.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum SnapshotPreviewBasis {
    /// Correct only if the supplied snapshots match their operation points,
    /// the stated API/profile premises hold, and predecessors complete normally.
    /// The materializer does not verify freshness or observe any execution.
    SuppliedSnapshotConditional,
}

/// The narrow program point described by a materialized artifact.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum SnapshotPreviewScope {
    /// Bytes after the selected operation succeeds, before any later effects.
    OperationPoint,
    /// Accepted owned stream content; physical visibility requires flush or close.
    LogicalStream,
}

/// A half-open UTF-8 byte range in an artifact endpoint, not in source code.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct SnapshotByteRange {
    /// Inclusive byte offset in the endpoint.
    pub start: usize,
    /// Exclusive byte offset in the endpoint.
    pub end: usize,
}

/// Replacing the before range with the after range reconstructs the new bytes.
/// A single enclosing change is emitted; minimal edit distance is not claimed.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct SnapshotReplacement {
    /// Range to remove from `before_bytes`, treating absence as empty bytes.
    pub before: SnapshotByteRange,
    /// Range to insert from `after_bytes`.
    pub after: SnapshotByteRange,
}

/// Language-specific source premises remain visible in the exact-byte view.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(
    tag = "language",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[non_exhaustive]
pub enum SnapshotPreviewDetails {
    /// Standard Python recipes and their source-level later-state uncertainty.
    Python {
        /// Conditional source/API hypotheses, never runtime attestations.
        assumptions: Vec<PythonAssumption>,
        /// Later Python source effects; no value establishes final disk state.
        final_state: PythonFinalState,
        /// Literal-loop occurrence provenance of this operation's source sink.
        iteration_context: Vec<PythonIterationContext>,
    },
    /// The separate finite Node CommonJS recipe registry.
    JavaScript {
        /// Conditional source/API hypotheses, never runtime attestations.
        assumptions: Vec<JavaScriptAssumption>,
    },
}

/// Exact endpoint bytes under supplied snapshot and execution premises.
/// This is neither an applied patch nor a final-state or successful-run claim.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ConditionalSnapshotPreview {
    /// Snapshot freshness, operation-point binding and normal completion remain
    /// caller premises; the materializer verifies identity consistency only.
    pub basis: SnapshotPreviewBasis,
    /// Selected operation point, explicitly separate from final file state.
    pub scope: SnapshotPreviewScope,
    /// Present only for a logical stream endpoint, never physical replay bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logical_stream: Option<super::LogicalStreamPreviewMetadata>,
    /// Call namespace shared by the analysis and supplied snapshots.
    pub call_id: CallId,
    /// Selected ordinal in the outer analysis operation sequence.
    pub ordinal: usize,
    /// Exact selected source sink evidence.
    pub evidence: Span,
    /// Exact spelling/cwd; filesystem aliases are not resolved.
    pub target: Target,
    /// Source-local mutation count immediately before this operation.
    pub filesystem_epoch: SnapshotEpoch,
    /// Supplied destination bytes, or a supplied assertion of absence.
    pub before_bytes: Option<Vec<u8>>,
    /// Conditional bytes after successful completion of this one operation.
    pub after_bytes: Vec<u8>,
    /// Exact reconstruction ranges; empty when endpoint bytes match.
    pub replacements: Vec<SnapshotReplacement>,
    /// Valid unified diff with one enclosing hunk and up to three context lines.
    /// Fixed endpoint labels avoid interpreting paths as diff syntax.
    pub unified_diff: String,
    /// Only reads actually consumed by the selected expression branch.
    pub read_bindings: Vec<SnapshotReadBinding>,
    /// Explicit caller-supplied text-I/O premises.
    pub text_profile: SnapshotTextProfile,
    /// Language-specific conditional premises and source context.
    pub details: SnapshotPreviewDetails,
    /// Shell/interpreter dispatch hypotheses retained from source analysis.
    pub carrier_assumptions: Vec<SelectiveCarrierAssumption>,
    /// Earlier modeled calls that must complete normally to reach the operation.
    pub predecessor_conditions: Vec<Span>,
    /// Later source uncertainty retained even when endpoint bytes are computed.
    pub later_context: SelectiveLaterContext,
}
