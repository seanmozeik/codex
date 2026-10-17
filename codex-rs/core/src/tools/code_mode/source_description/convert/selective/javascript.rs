//! Node conversion preserves its separate string semantics and runtime premises.

use super::Unsupported;
use super::activity;
use super::span;
use super::target;
use super::wire;

pub(super) fn prediction(
    value: &activity::JavaScriptEdit,
) -> Result<wire::SelectiveJavaScriptEdit, Unsupported> {
    Ok(wire::SelectiveJavaScriptEdit {
        ordinal: value.ordinal,
        evidence: span(&value.evidence),
        target: target(&value.target)?,
        content: recipe(&value.content, 0)?,
        write_effect: write_effect(&value.write_effect)?,
        dependencies: value
            .dependencies
            .iter()
            .map(dependency)
            .collect::<Result<Vec<_>, _>>()?,
        predecessor_conditions: value.predecessor_conditions.iter().map(span).collect(),
    })
}

fn write_effect(
    value: &activity::JavaScriptWriteEffect,
) -> Result<wire::SelectiveJavaScriptWriteEffect, Unsupported> {
    Ok(match value {
        activity::JavaScriptWriteEffect::Overwrite => {
            wire::SelectiveJavaScriptWriteEffect::Overwrite
        }
        activity::JavaScriptWriteEffect::Truncate {
            descriptor_id,
            opened_at,
        } => wire::SelectiveJavaScriptWriteEffect::Truncate {
            descriptor_id: *descriptor_id,
            opened_at: span(opened_at),
        },
        activity::JavaScriptWriteEffect::DescriptorAppend {
            descriptor_id,
            opened_at,
        } => wire::SelectiveJavaScriptWriteEffect::DescriptorAppend {
            descriptor_id: *descriptor_id,
            opened_at: span(opened_at),
        },
        activity::JavaScriptWriteEffect::DescriptorWrite {
            descriptor_id,
            opened_at,
            byte_offset,
            position,
        } => wire::SelectiveJavaScriptWriteEffect::DescriptorWrite {
            descriptor_id: *descriptor_id,
            opened_at: span(opened_at),
            byte_offset: *byte_offset,
            position: match position {
                activity::JavaScriptDescriptorPosition::Cursor => {
                    wire::SelectiveJavaScriptDescriptorPosition::Cursor
                }
                activity::JavaScriptDescriptorPosition::Absolute => {
                    wire::SelectiveJavaScriptDescriptorPosition::Absolute
                }
                _ => return Err(Unsupported),
            },
        },
        _ => return Err(Unsupported),
    })
}

pub(super) fn assumption(
    value: activity::JavaScriptAssumption,
) -> Result<wire::SelectiveJavaScriptAssumption, Unsupported> {
    Ok(match value {
        activity::JavaScriptAssumption::StandardNodeCommonJsEvaluation => {
            wire::SelectiveJavaScriptAssumption::StandardNodeCommonJsEvaluation
        }
        activity::JavaScriptAssumption::CanonicalNodeFs => {
            wire::SelectiveJavaScriptAssumption::CanonicalNodeFs
        }
        activity::JavaScriptAssumption::StandardStringMethods => {
            wire::SelectiveJavaScriptAssumption::StandardStringMethods
        }
        activity::JavaScriptAssumption::NoConcurrentMutation => {
            wire::SelectiveJavaScriptAssumption::NoConcurrentMutation
        }
        activity::JavaScriptAssumption::CompleteDescriptorWrites => {
            wire::SelectiveJavaScriptAssumption::CompleteDescriptorWrites
        }
        activity::JavaScriptAssumption::RegularDescriptorFiles => {
            wire::SelectiveJavaScriptAssumption::RegularDescriptorFiles
        }
        _ => return Err(Unsupported),
    })
}

fn dependency(
    value: &activity::JavaScriptDependency,
) -> Result<wire::SelectiveJavaScriptDependency, Unsupported> {
    Ok(match value {
        activity::JavaScriptDependency::Descriptor {
            descriptor_id,
            opened_at,
        } => wire::SelectiveJavaScriptDependency::Descriptor {
            descriptor_id: *descriptor_id,
            opened_at: span(opened_at),
        },
        activity::JavaScriptDependency::Binding {
            name,
            version,
            evidence,
        } => wire::SelectiveJavaScriptDependency::Binding {
            name: name.clone(),
            version: *version,
            evidence: span(evidence),
        },
        activity::JavaScriptDependency::ReadValue { read_id, evidence } => {
            wire::SelectiveJavaScriptDependency::ReadValue {
                read_id: *read_id,
                evidence: span(evidence),
            }
        }
        activity::JavaScriptDependency::EntryWorkingDirectory { cwd } => {
            wire::SelectiveJavaScriptDependency::EntryWorkingDirectory { cwd: cwd.clone() }
        }
        activity::JavaScriptDependency::FilesystemEpoch { epoch } => {
            wire::SelectiveJavaScriptDependency::FilesystemEpoch { epoch: *epoch }
        }
        activity::JavaScriptDependency::Capability { assumption: value } => {
            wire::SelectiveJavaScriptDependency::Capability {
                assumption: assumption(*value)?,
            }
        }
        _ => return Err(Unsupported),
    })
}

fn recipe(
    value: &activity::JavaScriptTextRecipe,
    depth: usize,
) -> Result<wire::SelectiveJavaScriptTextRecipe, Unsupported> {
    if depth > 32 {
        return Err(Unsupported);
    }
    Ok(match value {
        activity::JavaScriptTextRecipe::Literal { value, evidence } => {
            wire::SelectiveJavaScriptTextRecipe::Literal {
                value: value.clone(),
                evidence: span(evidence),
            }
        }
        activity::JavaScriptTextRecipe::ReadUtf8 {
            read_id,
            target: value,
            evidence,
            filesystem_epoch,
        } => wire::SelectiveJavaScriptTextRecipe::ReadUtf8 {
            read_id: *read_id,
            target: target(value)?,
            evidence: span(evidence),
            filesystem_epoch: *filesystem_epoch,
        },
        activity::JavaScriptTextRecipe::ReplaceFirst {
            input,
            old,
            new,
            evidence,
        } => wire::SelectiveJavaScriptTextRecipe::ReplaceFirst {
            input: Box::new(recipe(input, depth + 1)?),
            old: old.clone(),
            new: new.clone(),
            evidence: span(evidence),
        },
        activity::JavaScriptTextRecipe::ReplaceAll {
            input,
            old,
            new,
            evidence,
        } => wire::SelectiveJavaScriptTextRecipe::ReplaceAll {
            input: Box::new(recipe(input, depth + 1)?),
            old: old.clone(),
            new: new.clone(),
            evidence: span(evidence),
        },
        activity::JavaScriptTextRecipe::Slice {
            input,
            start,
            end,
            evidence,
        } => wire::SelectiveJavaScriptTextRecipe::Slice {
            input: Box::new(recipe(input, depth + 1)?),
            start: *start,
            end: *end,
            evidence: span(evidence),
        },
        activity::JavaScriptTextRecipe::Concat {
            left,
            right,
            evidence,
        } => wire::SelectiveJavaScriptTextRecipe::Concat {
            left: Box::new(recipe(left, depth + 1)?),
            right: Box::new(recipe(right, depth + 1)?),
            evidence: span(evidence),
        },
        _ => return Err(Unsupported),
    })
}

pub(super) fn barrier(
    value: &activity::JavaScriptBarrier,
) -> Result<wire::SelectiveBarrier, Unsupported> {
    Ok(wire::SelectiveBarrier {
        evidence: span(&value.evidence),
        reason: wire::SelectiveBarrierReason::JavaScript {
            reason: match value.reason {
                activity::JavaScriptBarrierReason::UnknownName => {
                    wire::SelectiveJavaScriptBarrierReason::UnknownName
                }
                activity::JavaScriptBarrierReason::UnmodeledCall => {
                    wire::SelectiveJavaScriptBarrierReason::UnmodeledCall
                }
                activity::JavaScriptBarrierReason::UnsupportedOperation => {
                    wire::SelectiveJavaScriptBarrierReason::UnsupportedOperation
                }
                _ => return Err(Unsupported),
            },
        },
    })
}
