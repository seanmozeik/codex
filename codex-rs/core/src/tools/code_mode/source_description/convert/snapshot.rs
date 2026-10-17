//! Automatic captures remain separate from source-only and observed file diffs.

use super::super::supplier::AutomaticSnapshot;
use super::logical;
use codex_code_activity as activity;
use codex_protocol::source_description as wire;

pub(super) fn matches_analysis(
    value: &AutomaticSnapshot,
    analysis: &activity::SelectiveEditAnalysis,
) -> bool {
    let preview = &value.preview;
    let Some(edit) = analysis.edits().get(preview.ordinal) else {
        return false;
    };
    let (target, epoch) = match &edit.details {
        activity::SelectiveEditDetails::Python { prediction, .. } => match &prediction.operation {
            activity::PythonOperation::LogicalWrite(write) => {
                if preview.scope != activity::SnapshotPreviewScope::LogicalStream
                    || !preview.logical_stream.as_ref().is_some_and(|metadata| {
                        metadata.binding.source_id == edit.evidence.source_id
                            && metadata.binding.stream_id == write.stream_id
                            && metadata.binding.opened_at == write.opened_at
                            && metadata.binding.base_filesystem_epoch.get()
                                == write.base_filesystem_epoch
                            && metadata.binding.revision == write.stream_revision
                            && metadata.mode == write.mode
                            && metadata.position == write.position
                    })
                {
                    return false;
                }
                (&prediction.target, write.base_filesystem_epoch)
            }
            _ => {
                if !physical_scope(preview) {
                    return false;
                }
                (&prediction.target, prediction.ordinal)
            }
        },
        activity::SelectiveEditDetails::JavaScript { prediction, .. } => {
            if !physical_scope(preview) {
                return false;
            }
            (&prediction.target, prediction.ordinal)
        }
        _ => return false,
    };
    preview.call_id == *analysis.call_id()
        && preview.ordinal == edit.ordinal
        && preview.evidence == edit.evidence
        && &preview.target == target
        && preview.filesystem_epoch.get() == epoch
}

fn physical_scope(preview: &activity::ConditionalSnapshotPreview) -> bool {
    preview.scope == activity::SnapshotPreviewScope::OperationPoint
        && preview.logical_stream.is_none()
}

pub(super) fn convert(value: &AutomaticSnapshot) -> Option<wire::SelectiveSnapshotPreview> {
    let preview = &value.preview;
    Some(wire::SelectiveSnapshotPreview {
        basis: wire::SelectiveSnapshotBasis::PreExecutionSnapshotConditional,
        scope: match preview.scope {
            activity::SnapshotPreviewScope::OperationPoint if preview.logical_stream.is_none() => {
                wire::SelectiveSnapshotScope::OperationPoint
            }
            activity::SnapshotPreviewScope::LogicalStream if preview.logical_stream.is_some() => {
                wire::SelectiveSnapshotScope::LogicalStream
            }
            _ => return None,
        },
        logical_stream: preview
            .logical_stream
            .as_ref()
            .map(logical::metadata)
            .transpose()
            .ok()?,
        source_id: preview.evidence.source_id.index(),
        filesystem_epoch: preview.filesystem_epoch.get(),
        before: preview
            .before_bytes
            .as_deref()
            .map(std::str::from_utf8)
            .transpose()
            .ok()?
            .map(str::to_owned),
        after: std::str::from_utf8(&preview.after_bytes).ok()?.to_owned(),
        unified_diff: preview.unified_diff.clone(),
        read_bindings: preview
            .read_bindings
            .iter()
            .map(binding)
            .collect::<Option<Vec<_>>>()?,
        text_profile: profile(preview.text_profile)?,
        environment_id: value.environment_id.clone(),
        resolved_cwd: value.resolved_cwd.inferred_native_path_string(),
        resolved_target: value.resolved_target.inferred_native_path_string(),
        replay_ordinals: value.replay_ordinals.clone(),
    })
}

fn binding(value: &activity::SnapshotReadBinding) -> Option<wire::SelectiveSnapshotReadBinding> {
    let activity::Target::Literal { path, cwd } = &value.target else {
        return None;
    };
    Some(wire::SelectiveSnapshotReadBinding {
        source_id: value.read.source_id.index(),
        read_id: value.read.read_id,
        filesystem_epoch: value.filesystem_epoch.get(),
        target: wire::SelectiveTarget {
            path: path.clone(),
            cwd: cwd.clone(),
        },
        encoding: match value.encoding {
            activity::TextEncoding::Utf8 => wire::SelectiveTextEncoding::Utf8,
            activity::TextEncoding::RuntimeDefault => wire::SelectiveTextEncoding::RuntimeDefault,
        },
        newline: read_newline(value.newline)?,
    })
}

fn profile(value: activity::SnapshotTextProfile) -> Option<wire::SelectiveSnapshotTextProfile> {
    Some(wire::SelectiveSnapshotTextProfile {
        read_newline: read_newline(value.read_newline)?,
        write_newline: match value.write_newline {
            activity::SnapshotWriteNewline::Unspecified => {
                wire::SelectiveSnapshotWriteNewline::Unspecified
            }
            activity::SnapshotWriteNewline::Lf => wire::SelectiveSnapshotWriteNewline::Lf,
            activity::SnapshotWriteNewline::Cr => wire::SelectiveSnapshotWriteNewline::Cr,
            activity::SnapshotWriteNewline::CrLf => wire::SelectiveSnapshotWriteNewline::CrLf,
            activity::SnapshotWriteNewline::Preserve => {
                wire::SelectiveSnapshotWriteNewline::Preserve
            }
            _ => return None,
        },
        runtime_default_encoding: match value.runtime_default_encoding {
            None => None,
            Some(activity::SnapshotEncoding::Utf8) => Some(wire::SelectiveSnapshotEncoding::Utf8),
            _ => return None,
        },
    })
}

fn read_newline(
    value: activity::SnapshotReadNewline,
) -> Option<wire::SelectiveSnapshotReadNewline> {
    Some(match value {
        activity::SnapshotReadNewline::Unspecified => {
            wire::SelectiveSnapshotReadNewline::Unspecified
        }
        activity::SnapshotReadNewline::PythonUniversal => {
            wire::SelectiveSnapshotReadNewline::PythonUniversal
        }
        activity::SnapshotReadNewline::Preserve => wire::SelectiveSnapshotReadNewline::Preserve,
        activity::SnapshotReadNewline::SourceDeclared => {
            wire::SelectiveSnapshotReadNewline::SourceDeclared
        }
        _ => return None,
    })
}
