//! Lossless conversion of source newline policies and logical stream identity.

use super::Unsupported;
use super::activity;
use super::span;
use super::wire;

pub(super) fn read_newline(
    value: activity::PythonReadNewline,
) -> Result<wire::SelectivePythonReadNewline, Unsupported> {
    Ok(match value {
        activity::PythonReadNewline::UniversalTranslate => {
            wire::SelectivePythonReadNewline::UniversalTranslate
        }
        activity::PythonReadNewline::Preserve => wire::SelectivePythonReadNewline::Preserve,
        _ => return Err(Unsupported),
    })
}

pub(super) fn write_newline(
    value: activity::PythonWriteNewline,
) -> Result<wire::SelectivePythonWriteNewline, Unsupported> {
    Ok(match value {
        activity::PythonWriteNewline::RuntimeDefault => {
            wire::SelectivePythonWriteNewline::RuntimeDefault
        }
        activity::PythonWriteNewline::Preserve => wire::SelectivePythonWriteNewline::Preserve,
        activity::PythonWriteNewline::Cr => wire::SelectivePythonWriteNewline::Cr,
        activity::PythonWriteNewline::CrLf => wire::SelectivePythonWriteNewline::CrLf,
        _ => return Err(Unsupported),
    })
}

pub(super) fn mode(
    value: activity::PythonStreamMode,
) -> Result<wire::SelectivePythonStreamMode, Unsupported> {
    Ok(match value {
        activity::PythonStreamMode::TruncateWrite => wire::SelectivePythonStreamMode::TruncateWrite,
        activity::PythonStreamMode::UpdateExisting => {
            wire::SelectivePythonStreamMode::UpdateExisting
        }
        _ => return Err(Unsupported),
    })
}

pub(super) fn position(
    value: &activity::PythonStreamPosition,
) -> Result<wire::SelectivePythonStreamPosition, Unsupported> {
    Ok(match value {
        activity::PythonStreamPosition::InitialZero => {
            wire::SelectivePythonStreamPosition::InitialZero
        }
        activity::PythonStreamPosition::RewoundZero {
            read_id,
            read_evidence,
            seek_evidence,
        } => wire::SelectivePythonStreamPosition::RewoundZero {
            read_id: *read_id,
            read_evidence: span(read_evidence),
            seek_evidence: span(seek_evidence),
        },
        _ => return Err(Unsupported),
    })
}

pub(super) fn result_condition(
    value: activity::PythonWriteResultCondition,
) -> Result<wire::SelectivePythonWriteResultCondition, Unsupported> {
    match value {
        activity::PythonWriteResultCondition::FullCharacterCount => {
            Ok(wire::SelectivePythonWriteResultCondition::FullCharacterCount)
        }
        _ => Err(Unsupported),
    }
}

pub(super) fn metadata(
    value: &activity::LogicalStreamPreviewMetadata,
) -> Result<wire::SelectiveLogicalStreamPreviewMetadata, Unsupported> {
    Ok(wire::SelectiveLogicalStreamPreviewMetadata {
        binding: wire::SelectiveLogicalStreamBinding {
            source_id: value.binding.source_id.index(),
            stream_id: wire::SelectivePythonStreamId::new(value.binding.stream_id.get()),
            opened_at: span(&value.binding.opened_at),
            base_filesystem_epoch: value.binding.base_filesystem_epoch.get(),
            revision: value.binding.revision,
        },
        mode: mode(value.mode)?,
        position: position(&value.position)?,
        revision_after: value.revision_after,
        payload_bytes: value.payload_bytes.clone(),
        accepted_characters_required: value.accepted_characters_required,
        realization: match value.realization {
            activity::LogicalStreamRealization::RequiresSuccessfulFlushOrClose => {
                wire::SelectiveLogicalStreamRealization::RequiresSuccessfulFlushOrClose
            }
            _ => return Err(Unsupported),
        },
    })
}
