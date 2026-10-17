//! Keep logical stream bindings separate from physical snapshot requests.

use crate::session::turn_context::TurnEnvironment;
use codex_code_activity as activity;

pub(super) enum BoundInput {
    Physical(activity::SnapshotPreviewInput),
    Logical(activity::LogicalStreamPreviewInput),
}

impl BoundInput {
    pub(super) fn new(
        input: activity::SnapshotPreviewInput,
        edit: &activity::PredictedEdit,
    ) -> Self {
        if let activity::SelectiveEditDetails::Python { prediction, .. } = &edit.details
            && let activity::PythonOperation::LogicalWrite(write) = &prediction.operation
        {
            return Self::Logical(activity::LogicalStreamPreviewInput::new(
                input,
                activity::LogicalStreamBinding::new(
                    edit.evidence.source_id,
                    write.stream_id,
                    write.opened_at.clone(),
                    activity::SnapshotEpoch::new(write.base_filesystem_epoch),
                    write.stream_revision,
                ),
            ));
        }
        Self::Physical(input)
    }

    pub(super) fn snapshot_mut(&mut self) -> &mut activity::SnapshotPreviewInput {
        match self {
            Self::Physical(input) => input,
            Self::Logical(input) => &mut input.snapshot,
        }
    }

    pub(super) fn materialize(
        &self,
        analysis: &activity::SelectiveEditAnalysis,
        ordinal: usize,
    ) -> Result<activity::ConditionalSnapshotPreview, activity::SnapshotPreviewError> {
        match self {
            Self::Physical(input) => {
                activity::materialize_snapshot_preview(analysis, ordinal, input)
            }
            Self::Logical(input) => {
                activity::materialize_logical_stream_preview(analysis, ordinal, input)
            }
        }
    }
}

pub(super) fn operation<'a>(
    edit: &'a activity::PredictedEdit,
    environment: &TurnEnvironment,
) -> Option<(usize, &'a activity::Target, activity::SnapshotTextProfile)> {
    match &edit.details {
        activity::SelectiveEditDetails::Python { prediction, .. } => {
            let (epoch, newline) = match &prediction.operation {
                activity::PythonOperation::Truncate => {
                    // Emptying a file does not encode text or translate newlines.
                    return Some((
                        prediction.ordinal,
                        &prediction.target,
                        activity::SnapshotTextProfile::new(
                            activity::SnapshotReadNewline::SourceDeclared,
                            activity::SnapshotWriteNewline::Preserve,
                        ),
                    ));
                }
                activity::PythonOperation::WriteText { newline, .. } => {
                    (prediction.ordinal, *newline)
                }
                activity::PythonOperation::LogicalWrite(write) => {
                    (write.base_filesystem_epoch, Some(write.newline))
                }
            };
            Some((
                epoch,
                &prediction.target,
                activity::SnapshotTextProfile::new(
                    activity::SnapshotReadNewline::SourceDeclared,
                    output_policy(newline, environment)?,
                ),
            ))
        }
        activity::SelectiveEditDetails::JavaScript { prediction, .. } => Some((
            prediction.ordinal,
            &prediction.target,
            activity::SnapshotTextProfile::new(
                activity::SnapshotReadNewline::Preserve,
                activity::SnapshotWriteNewline::Preserve,
            ),
        )),
        _ => None,
    }
}

fn output_policy(
    value: Option<activity::PythonWriteNewline>,
    environment: &TurnEnvironment,
) -> Option<activity::SnapshotWriteNewline> {
    match value {
        Some(activity::PythonWriteNewline::Preserve) => {
            Some(activity::SnapshotWriteNewline::Preserve)
        }
        Some(activity::PythonWriteNewline::Cr) => Some(activity::SnapshotWriteNewline::Cr),
        Some(activity::PythonWriteNewline::CrLf) => Some(activity::SnapshotWriteNewline::CrLf),
        None | Some(activity::PythonWriteNewline::RuntimeDefault) => {
            let os = environment
                .executor_platform_os
                .as_deref()
                .or_else(|| (!environment.environment.is_remote()).then_some(std::env::consts::OS));
            match os {
                Some("windows") => Some(activity::SnapshotWriteNewline::CrLf),
                Some("linux" | "macos" | "freebsd" | "openbsd" | "netbsd") => {
                    Some(activity::SnapshotWriteNewline::Lf)
                }
                _ => None,
            }
        }
        _ => None,
    }
}
