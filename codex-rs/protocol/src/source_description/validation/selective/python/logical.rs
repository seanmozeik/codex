//! Structural stream ownership and predecessor ordering, not execution proof.
use super::super::Budget;
use super::super::MAX_SELECTIVE_NODES;
use crate::source_description::SelectivePythonDependency;
use crate::source_description::SelectivePythonEdit;
use crate::source_description::SelectivePythonLogicalWrite;
use crate::source_description::SelectivePythonStreamPosition;
use crate::source_description::SelectiveTextEncoding;
use crate::source_description::SourceDescriptionSource;

pub(super) fn valid(
    prediction: &SelectivePythonEdit,
    write: &SelectivePythonLogicalWrite,
    sources: &[SourceDescriptionSource],
    budget: &mut Budget,
) -> bool {
    let open = &write.opened_at;
    if write.stream_id.get() >= MAX_SELECTIVE_NODES
        || write.stream_revision != 0
        || write.base_filesystem_epoch > prediction.ordinal
        || write.encoding != SelectiveTextEncoding::Utf8
        || !prediction.iteration_context.is_empty()
        || open.source_id != prediction.evidence.source_id
        || open.end_byte > prediction.evidence.start_byte
        || !budget.span(open, sources)
        || !prediction.predecessor_conditions.contains(open)
    {
        return false;
    }
    match &write.position {
        SelectivePythonStreamPosition::InitialZero => true,
        SelectivePythonStreamPosition::RewoundZero { read_id, read_evidence, seek_evidence } => {
            *read_id < MAX_SELECTIVE_NODES
                && read_evidence.source_id == open.source_id
                && seek_evidence.source_id == open.source_id
                && open.end_byte <= read_evidence.start_byte
                && read_evidence.end_byte <= seek_evidence.start_byte
                && seek_evidence.end_byte <= prediction.evidence.start_byte
                && budget.span(read_evidence, sources)
                && budget.span(seek_evidence, sources)
                && prediction.predecessor_conditions.iter().position(|p| p == open)
                    < prediction.predecessor_conditions.iter().position(|p| p == read_evidence)
                && prediction.predecessor_conditions.iter().position(|p| p == read_evidence)
                    < prediction.predecessor_conditions.iter().position(|p| p == seek_evidence)
                && prediction.dependencies.iter().any(|d| matches!(d, SelectivePythonDependency::ReadValue { read_id: id, evidence } if id == read_id && evidence == read_evidence))
        }
    }
}
