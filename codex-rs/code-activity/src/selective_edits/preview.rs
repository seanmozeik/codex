//! Pure bounded snapshot materialization, separate from runtime observation.
//!
//! Only caller-supplied bytes are inspected. No path is opened, no source is
//! executed, and no successful execution or final file state is inferred.

mod budget;
mod diff;
mod error;
mod input;
mod javascript;
mod logical;
mod logical_model;
mod model;
mod python;
mod snapshots;
mod text;

pub use budget::SnapshotPreviewLimits;
pub use error::SnapshotPreviewError;
pub use error::SnapshotPreviewLimit;
pub use input::SnapshotEncoding;
pub use input::SnapshotEpoch;
pub use input::SnapshotPreviewInput;
pub use input::SnapshotReadBinding;
pub use input::SnapshotReadNewline;
pub use input::SnapshotTargetBinding;
pub use input::SnapshotTextProfile;
pub use input::SnapshotWriteNewline;
pub use input::SourceReadId;
pub use input::TrustedReadSnapshot;
pub use input::TrustedTargetSnapshot;
pub use logical::materialize_logical_stream_preview;
pub use logical_model::LogicalStreamBinding;
pub use logical_model::LogicalStreamPreviewInput;
pub use logical_model::LogicalStreamPreviewMetadata;
pub use logical_model::LogicalStreamRealization;
pub use model::ConditionalSnapshotPreview;
pub use model::SnapshotByteRange;
pub use model::SnapshotPreviewBasis;
pub use model::SnapshotPreviewDetails;
pub use model::SnapshotPreviewScope;
pub use model::SnapshotReplacement;

use super::PythonOperation;
use super::SelectiveEditAnalysis;
use super::SelectiveEditDetails;
use super::SelectiveEditDisposition;
use budget::Budget;
use python::PythonEvaluator;
use snapshots::Snapshots;

/// Materializes one operation using trusted, exactly bound supplied snapshots.
///
/// `ordinal` selects an outer analysis operation. Destination epochs and read
/// IDs remain local to that operation's source. A caller must obtain and vouch
/// for the correct operation-point snapshots; self-described metadata does not
/// prove freshness, path identity, successful execution, or API behavior.
///
/// Python uses Unicode scalar/code-point indices and explicitly supplied
/// universal-read and host-write newline semantics. The separate Node registry
/// preserves UTF-8 newlines and distinguishes first-match from all-match
/// primitive-string replacement recipes. Node slices use UTF-16 code-unit
/// indices and refuse nonempty results containing unpaired surrogates.
/// Invalid UTF-8 is refused for both languages. Only a selected conditional arm
/// is evaluated. Later uncertainty is retained and does not become a final-state
/// assertion. Diffs use one enclosing hunk rather than minimum edit distance.
///
/// # Errors
///
/// Returns a typed [`SnapshotPreviewError`] for an unavailable operation,
/// missing/duplicate/mismatched/contradictory snapshots, unresolved codec or
/// newline premises, a modeled string exception, or a resource-limit refusal.
/// Resource checks precede allocations and no partial artifact is returned.
pub fn materialize_snapshot_preview(
    analysis: &SelectiveEditAnalysis,
    ordinal: usize,
    input: &SnapshotPreviewInput,
) -> Result<ConditionalSnapshotPreview, SnapshotPreviewError> {
    let mut budget = Budget::new(input.limits);
    budget.input(input.call_id.as_str().len())?;
    if &input.call_id != analysis.call_id() {
        return Err(SnapshotPreviewError::CallMismatch);
    }
    if analysis.disposition() != SelectiveEditDisposition::Analyzed {
        return Err(SnapshotPreviewError::OperationUnavailable);
    }
    let edit = analysis
        .edits()
        .get(ordinal)
        .ok_or(SnapshotPreviewError::OperationUnavailable)?;
    let (target, local_ordinal) = match &edit.details {
        SelectiveEditDetails::Python { prediction, .. } => {
            if matches!(&prediction.operation, PythonOperation::LogicalWrite(_)) {
                return Err(SnapshotPreviewError::UnsupportedOperation);
            }
            (&prediction.target, prediction.ordinal)
        }
        SelectiveEditDetails::JavaScript { prediction, .. } => {
            (&prediction.target, prediction.ordinal)
        }
        SelectiveEditDetails::Patch { .. } => {
            return Err(SnapshotPreviewError::UnsupportedOperation);
        }
    };
    let epoch = SnapshotEpoch::new(local_ordinal);
    let mut snapshots = Snapshots::new(input, edit.evidence.source_id, epoch, target, &mut budget)?;
    let before = input
        .before
        .bytes
        .as_deref()
        .map(std::str::from_utf8)
        .transpose()
        .map_err(|_| SnapshotPreviewError::InvalidUtf8)?;
    let mut predecessor_conditions = edit.predecessor_conditions.clone();
    let (after, details) = match &edit.details {
        SelectiveEditDetails::Python {
            prediction,
            assumptions,
        } => {
            predecessor_conditions.extend_from_slice(&prediction.predecessor_conditions);
            let after = match &prediction.operation {
                PythonOperation::Truncate => String::new(),
                PythonOperation::LogicalWrite(_) => {
                    return Err(SnapshotPreviewError::UnsupportedOperation);
                }
                PythonOperation::WriteText {
                    content,
                    encoding,
                    newline,
                } => {
                    let text = PythonEvaluator {
                        snapshots: &mut snapshots,
                        budget: &mut budget,
                    }
                    .text(content, 1)?;
                    snapshots.python_write(text, *encoding, *newline, &mut budget)?
                }
            };
            (
                after,
                SnapshotPreviewDetails::Python {
                    assumptions: assumptions.clone(),
                    final_state: prediction.final_state,
                    iteration_context: prediction.iteration_context.clone(),
                },
            )
        }
        SelectiveEditDetails::JavaScript {
            prediction,
            assumptions,
        } => {
            predecessor_conditions.extend_from_slice(&prediction.predecessor_conditions);
            if input.text_profile.write_newline != SnapshotWriteNewline::Preserve {
                return Err(SnapshotPreviewError::NewlineProfileMismatch);
            }
            let after = javascript::materialize(prediction, before, &mut snapshots, &mut budget)?;
            (
                after,
                SnapshotPreviewDetails::JavaScript {
                    assumptions: assumptions.clone(),
                },
            )
        }
        SelectiveEditDetails::Patch { .. } => {
            return Err(SnapshotPreviewError::UnsupportedOperation);
        }
    };
    let (replacements, unified_diff) = diff::compare(before, &after, &mut budget)?;
    budget.work(before.map_or(0, str::len))?;
    Ok(ConditionalSnapshotPreview {
        basis: SnapshotPreviewBasis::SuppliedSnapshotConditional,
        scope: SnapshotPreviewScope::OperationPoint,
        logical_stream: None,
        call_id: input.call_id.clone(),
        ordinal,
        evidence: edit.evidence.clone(),
        target: target.clone(),
        filesystem_epoch: epoch,
        before_bytes: input.before.bytes.clone(),
        after_bytes: after.into_bytes(),
        replacements,
        unified_diff,
        read_bindings: snapshots.used,
        text_profile: input.text_profile,
        details,
        carrier_assumptions: edit.carrier_assumptions.clone(),
        predecessor_conditions,
        later_context: edit.later_context,
    })
}
