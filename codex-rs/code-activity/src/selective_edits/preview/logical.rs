//! Materialize a logical endpoint without predicting immediate disk mutation.

use super::ConditionalSnapshotPreview;
use super::LogicalStreamBinding;
use super::LogicalStreamPreviewInput;
use super::LogicalStreamPreviewMetadata;
use super::LogicalStreamRealization;
use super::SnapshotEpoch;
use super::SnapshotPreviewBasis;
use super::SnapshotPreviewDetails;
use super::SnapshotPreviewError;
use super::SnapshotPreviewScope;
use super::budget::Budget;
use super::diff;
use super::python::PythonEvaluator;
use super::snapshots::Snapshots;
use crate::selective_edits::PythonOperation;
use crate::selective_edits::PythonStreamMode;
use crate::selective_edits::SelectiveEditAnalysis;
use crate::selective_edits::SelectiveEditDetails;
use crate::selective_edits::SelectiveEditDisposition;

/// Materializes one conditional owned UTF-8 logical-stream write from supplied bytes.
///
/// This entry point never performs I/O. Full character acceptance is a premise,
/// and physical realization still requires successful flush or close. Returned
/// bytes must not be replayed as filesystem snapshots for subsequent operations.
///
/// # Errors
/// Returns a typed refusal for a nonlogical operation, mismatched stream/seed/read
/// identity, missing or contradictory snapshots, invalid UTF-8, incompatible
/// newline profiles, recipe exceptions or resource ceilings. No partial artifact
/// is returned. A truncate-mode seed must be present and already empty.
pub fn materialize_logical_stream_preview(
    analysis: &SelectiveEditAnalysis,
    ordinal: usize,
    input: &LogicalStreamPreviewInput,
) -> Result<ConditionalSnapshotPreview, SnapshotPreviewError> {
    let snapshot = &input.snapshot;
    let mut budget = Budget::new(snapshot.limits);
    budget.input(snapshot.call_id.as_str().len())?;
    if &snapshot.call_id != analysis.call_id() {
        return Err(SnapshotPreviewError::CallMismatch);
    }
    if analysis.disposition() != SelectiveEditDisposition::Analyzed {
        return Err(SnapshotPreviewError::OperationUnavailable);
    }
    let edit = analysis
        .edits()
        .get(ordinal)
        .ok_or(SnapshotPreviewError::OperationUnavailable)?;
    let SelectiveEditDetails::Python {
        prediction,
        assumptions,
    } = &edit.details
    else {
        return Err(SnapshotPreviewError::UnsupportedOperation);
    };
    let PythonOperation::LogicalWrite(write) = &prediction.operation else {
        return Err(SnapshotPreviewError::UnsupportedOperation);
    };
    let epoch = SnapshotEpoch::new(write.base_filesystem_epoch);
    let expected = LogicalStreamBinding::new(
        edit.evidence.source_id,
        write.stream_id,
        write.opened_at.clone(),
        epoch,
        write.stream_revision,
    );
    if input.binding != expected {
        return Err(SnapshotPreviewError::LogicalStreamBindingMismatch);
    }
    let mut snapshots = Snapshots::new(
        snapshot,
        edit.evidence.source_id,
        epoch,
        &prediction.target,
        &mut budget,
    )?;
    let bytes = snapshot
        .before
        .bytes
        .as_deref()
        .ok_or(SnapshotPreviewError::MissingLogicalStreamPreimage)?;
    budget.work(bytes.len())?;
    let before = std::str::from_utf8(bytes).map_err(|_| SnapshotPreviewError::InvalidUtf8)?;
    if write.mode == PythonStreamMode::TruncateWrite && !before.is_empty() {
        return Err(SnapshotPreviewError::LogicalStreamSeedMismatch);
    }
    let text = PythonEvaluator {
        snapshots: &mut snapshots,
        budget: &mut budget,
    }
    .text(&write.content, 1)?;
    budget.work(text.len())?;
    let accepted_characters_required = text.chars().count();
    let payload = snapshots.python_write(text, write.encoding, Some(write.newline), &mut budget)?;
    let after_len = before.len().max(payload.len());
    budget.output(after_len)?;
    let mut after = String::with_capacity(after_len);
    after.push_str(&payload);
    if payload.len() < before.len() {
        if !before.is_char_boundary(payload.len()) {
            return Err(SnapshotPreviewError::InvalidUtf8);
        }
        after.push_str(&before[payload.len()..]);
    }
    let (replacements, unified_diff) = diff::compare(Some(before), &after, &mut budget)?;
    budget.work(before.len())?;
    budget.work(
        edit.predecessor_conditions
            .len()
            .saturating_add(prediction.predecessor_conditions.len())
            .saturating_mul(64),
    )?;
    let mut predecessor_conditions = edit.predecessor_conditions.clone();
    predecessor_conditions.extend_from_slice(&prediction.predecessor_conditions);
    Ok(ConditionalSnapshotPreview {
        basis: SnapshotPreviewBasis::SuppliedSnapshotConditional,
        scope: SnapshotPreviewScope::LogicalStream,
        logical_stream: Some(LogicalStreamPreviewMetadata {
            binding: expected,
            mode: write.mode,
            position: write.position.clone(),
            revision_after: write.stream_revision + 1,
            payload_bytes: payload.into_bytes(),
            accepted_characters_required,
            realization: LogicalStreamRealization::RequiresSuccessfulFlushOrClose,
        }),
        call_id: snapshot.call_id.clone(),
        ordinal,
        evidence: edit.evidence.clone(),
        target: prediction.target.clone(),
        filesystem_epoch: epoch,
        before_bytes: snapshot.before.bytes.clone(),
        after_bytes: after.into_bytes(),
        replacements,
        unified_diff,
        read_bindings: snapshots.used,
        text_profile: snapshot.text_profile,
        details: SnapshotPreviewDetails::Python {
            assumptions: assumptions.clone(),
            final_state: prediction.final_state,
            iteration_context: prediction.iteration_context.clone(),
        },
        carrier_assumptions: edit.carrier_assumptions.clone(),
        predecessor_conditions,
        later_context: edit.later_context,
    })
}
