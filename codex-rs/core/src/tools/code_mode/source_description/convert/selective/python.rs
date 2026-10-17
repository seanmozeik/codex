//! Lossless Python operation and occurrence conversion.

use super::super::logical;
use super::Unsupported;
use super::activity;
use super::recipes::recipe;
use super::span;
use super::target;
use super::wire;

pub(super) fn python(
    value: &activity::PythonEdit,
) -> Result<wire::SelectivePythonEdit, Unsupported> {
    Ok(wire::SelectivePythonEdit {
        ordinal: value.ordinal,
        iteration_context: value
            .iteration_context
            .iter()
            .map(|iteration| wire::SelectivePythonIterationContext {
                evidence: span(&iteration.evidence),
                index: iteration.index,
                literal_count: iteration.literal_count,
            })
            .collect(),
        evidence: span(&value.evidence),
        target: target(&value.target)?,
        operation: match &value.operation {
            activity::PythonOperation::Truncate => wire::SelectivePythonOperation::Truncate,
            activity::PythonOperation::WriteText {
                content,
                encoding: value,
                newline,
            } => wire::SelectivePythonOperation::WriteText {
                content: recipe(content, 0)?,
                encoding: encoding(*value),
                newline: newline.map(logical::write_newline).transpose()?,
            },
            activity::PythonOperation::LogicalWrite(write) => {
                wire::SelectivePythonOperation::LogicalWrite(wire::SelectivePythonLogicalWrite {
                    stream_id: wire::SelectivePythonStreamId::new(write.stream_id.get()),
                    opened_at: span(&write.opened_at),
                    mode: logical::mode(write.mode)?,
                    position: logical::position(&write.position)?,
                    base_filesystem_epoch: write.base_filesystem_epoch,
                    stream_revision: write.stream_revision,
                    content: recipe(&write.content, 0)?,
                    encoding: encoding(write.encoding),
                    newline: logical::write_newline(write.newline)?,
                    required_result: logical::result_condition(write.required_result)?,
                })
            }
        },
        dependencies: value
            .dependencies
            .iter()
            .map(|value| {
                Ok(match value {
                    activity::PythonDependency::Binding {
                        name,
                        version,
                        evidence,
                    } => wire::SelectivePythonDependency::Binding {
                        name: name.clone(),
                        version: *version,
                        evidence: span(evidence),
                    },
                    activity::PythonDependency::ReadValue { read_id, evidence } => {
                        wire::SelectivePythonDependency::ReadValue {
                            read_id: *read_id,
                            evidence: span(evidence),
                        }
                    }
                    activity::PythonDependency::EntryWorkingDirectory { cwd } => {
                        wire::SelectivePythonDependency::EntryWorkingDirectory { cwd: cwd.clone() }
                    }
                    activity::PythonDependency::Capability { assumption: value } => {
                        wire::SelectivePythonDependency::Capability {
                            assumption: assumption(*value),
                        }
                    }
                    activity::PythonDependency::FilesystemEpoch { epoch } => {
                        wire::SelectivePythonDependency::FilesystemEpoch { epoch: *epoch }
                    }
                })
            })
            .collect::<Result<Vec<_>, Unsupported>>()?,
        predecessor_conditions: value.predecessor_conditions.iter().map(span).collect(),
        final_state: match value.final_state {
            activity::PythonFinalState::NotEstablished => {
                wire::SelectivePythonFinalState::NotEstablished
            }
            activity::PythonFinalState::KnownLaterTouch => {
                wire::SelectivePythonFinalState::KnownLaterTouch
            }
            activity::PythonFinalState::PossibleAliasLaterTouch => {
                wire::SelectivePythonFinalState::PossibleAliasLaterTouch
            }
            activity::PythonFinalState::OpaqueSuccessor => {
                wire::SelectivePythonFinalState::OpaqueSuccessor
            }
        },
    })
}

pub(super) fn encoding(value: activity::TextEncoding) -> wire::SelectiveTextEncoding {
    match value {
        activity::TextEncoding::Utf8 => wire::SelectiveTextEncoding::Utf8,
        activity::TextEncoding::RuntimeDefault => wire::SelectiveTextEncoding::RuntimeDefault,
    }
}

pub(super) fn assumption(value: activity::PythonAssumption) -> wire::SelectivePythonAssumption {
    match value {
        activity::PythonAssumption::StandardPythonEvaluation => {
            wire::SelectivePythonAssumption::StandardPythonEvaluation
        }
        activity::PythonAssumption::StandardBuiltinOpen => {
            wire::SelectivePythonAssumption::StandardBuiltinOpen
        }
        activity::PythonAssumption::CanonicalPathlib => {
            wire::SelectivePythonAssumption::CanonicalPathlib
        }
        activity::PythonAssumption::StandardTextMethods => {
            wire::SelectivePythonAssumption::StandardTextMethods
        }
        activity::PythonAssumption::NoConcurrentMutation => {
            wire::SelectivePythonAssumption::NoConcurrentMutation
        }
        activity::PythonAssumption::PathlibNewlineSupport => {
            wire::SelectivePythonAssumption::PathlibNewlineSupport
        }
    }
}
