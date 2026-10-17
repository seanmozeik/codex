//! Node recipes keep replacement modes and their explicit CommonJS premise.

use super::cwd_label;
use super::location;
use super::target_label;
use codex_protocol::source_description::SelectiveJavaScriptAssumption;
use codex_protocol::source_description::SelectiveJavaScriptBarrierReason;
use codex_protocol::source_description::SelectiveJavaScriptDependency;
use codex_protocol::source_description::SelectiveJavaScriptDescriptorPosition;
use codex_protocol::source_description::SelectiveJavaScriptEdit;
use codex_protocol::source_description::SelectiveJavaScriptTextRecipe;
use codex_protocol::source_description::SelectiveJavaScriptWriteEffect;
use ratatui::text::Line;

pub(super) fn detail(
    prediction: &SelectiveJavaScriptEdit,
    assumptions: &[SelectiveJavaScriptAssumption],
    lines: &mut Vec<Line<'static>>,
) {
    lines.push(Line::from(format!(
        "Target: {}",
        target_label(&prediction.target)
    )));
    lines.push(Line::from(operation_label(prediction)));
    recipe_lines(&prediction.content, "  ", lines);
    for assumption in assumptions {
        lines.push(Line::from(format!(
            "Assume {}",
            assumption_label(*assumption)
        )));
    }
    for condition in &prediction.predecessor_conditions {
        lines.push(Line::from(format!(
            "Requires earlier Node operation to complete normally · {}",
            location(condition)
        )));
    }
    for dependency in &prediction.dependencies {
        let value = match dependency {
            SelectiveJavaScriptDependency::Descriptor {
                descriptor_id,
                opened_at,
            } => format!(
                "descriptor {descriptor_id} opened at {}",
                location(opened_at)
            ),
            SelectiveJavaScriptDependency::Binding {
                name,
                version,
                evidence,
            } => format!(
                "binding {name:?} version {version} · {}",
                location(evidence)
            ),
            SelectiveJavaScriptDependency::ReadValue { read_id, evidence } => format!(
                "read {}:{read_id} · {}",
                evidence.source_id,
                location(evidence)
            ),
            SelectiveJavaScriptDependency::EntryWorkingDirectory { cwd } => {
                format!("entry cwd {}", cwd_label(cwd.as_deref()))
            }
            SelectiveJavaScriptDependency::FilesystemEpoch { epoch } => format!(
                "filesystem epoch {epoch} within source {}",
                prediction.evidence.source_id
            ),
            SelectiveJavaScriptDependency::Capability { assumption } => {
                assumption_label(*assumption).to_owned()
            }
        };
        lines.push(Line::from(format!("Depends on {value}")));
    }
}

pub(super) fn operation_label(prediction: &SelectiveJavaScriptEdit) -> String {
    match &prediction.write_effect {
        SelectiveJavaScriptWriteEffect::Overwrite => {
            "Operation: synchronous Node text overwrite · UTF-8".to_owned()
        }
        SelectiveJavaScriptWriteEffect::Truncate { descriptor_id, .. } => {
            format!("Operation: truncate at descriptor {descriptor_id} open")
        }
        SelectiveJavaScriptWriteEffect::DescriptorAppend { descriptor_id, .. } => {
            format!(
                "Operation: conditional descriptor {descriptor_id} UTF-8 append at operation-time EOF"
            )
        }
        SelectiveJavaScriptWriteEffect::DescriptorWrite {
            descriptor_id,
            byte_offset,
            position,
            ..
        } => {
            let position = match position {
                SelectiveJavaScriptDescriptorPosition::Cursor => "advance cursor",
                SelectiveJavaScriptDescriptorPosition::Absolute => "preserve cursor",
            };
            format!(
                "Operation: descriptor {descriptor_id} UTF-8 write at byte {byte_offset} · preserve suffix · {position}"
            )
        }
    }
}

fn recipe_lines(
    recipe: &SelectiveJavaScriptTextRecipe,
    indent: &str,
    lines: &mut Vec<Line<'static>>,
) {
    match recipe {
        SelectiveJavaScriptTextRecipe::Literal { value, evidence } => lines.push(Line::from(
            format!("{indent}Literal {value:?} · {}", location(evidence)),
        )),
        SelectiveJavaScriptTextRecipe::ReadUtf8 {
            read_id,
            target,
            evidence,
            filesystem_epoch,
        } => {
            lines.push(Line::from(format!("{indent}Node UTF-8 read {}:{read_id} at source-local filesystem epoch {filesystem_epoch} · {} · {}", evidence.source_id, target_label(target), location(evidence))));
        }
        SelectiveJavaScriptTextRecipe::ReplaceFirst {
            input,
            old,
            new,
            evidence,
        } => {
            lines.push(Line::from(format!(
                "{indent}Replace first {old:?} with literal {new:?} · {}",
                location(evidence)
            )));
            recipe_lines(input, &format!("{indent}  "), lines);
        }
        SelectiveJavaScriptTextRecipe::ReplaceAll {
            input,
            old,
            new,
            evidence,
        } => {
            lines.push(Line::from(format!(
                "{indent}Replace all {old:?} with literal {new:?} · {}",
                location(evidence)
            )));
            recipe_lines(input, &format!("{indent}  "), lines);
        }
        SelectiveJavaScriptTextRecipe::Slice {
            input,
            start,
            end,
            evidence,
        } => {
            let end = end.map_or_else(|| "length".to_owned(), |end| end.to_string());
            lines.push(Line::from(format!(
                "{indent}Node UTF-16 slice {start}..{end} · clamp to length · {}",
                location(evidence)
            )));
            recipe_lines(input, &format!("{indent}  "), lines);
        }
        SelectiveJavaScriptTextRecipe::Concat {
            left,
            right,
            evidence,
        } => {
            lines.push(Line::from(format!(
                "{indent}Concatenate left then right · {}",
                location(evidence)
            )));
            let indent = format!("{indent}  ");
            recipe_lines(left, &indent, lines);
            recipe_lines(right, &indent, lines);
        }
    }
}

fn assumption_label(assumption: SelectiveJavaScriptAssumption) -> &'static str {
    match assumption {
        SelectiveJavaScriptAssumption::StandardNodeCommonJsEvaluation => {
            "standard Node CommonJS evaluation"
        }
        SelectiveJavaScriptAssumption::CanonicalNodeFs => "canonical Node fs",
        SelectiveJavaScriptAssumption::StandardStringMethods => {
            "standard JavaScript string methods"
        }
        SelectiveJavaScriptAssumption::NoConcurrentMutation => "no concurrent mutation",
        SelectiveJavaScriptAssumption::CompleteDescriptorWrites => {
            "each descriptor write completes all requested bytes"
        }
        SelectiveJavaScriptAssumption::RegularDescriptorFiles => {
            "descriptors refer to regular files"
        }
    }
}

pub(super) fn barrier_label(reason: SelectiveJavaScriptBarrierReason) -> &'static str {
    match reason {
        SelectiveJavaScriptBarrierReason::UnknownName => "unknown Node name",
        SelectiveJavaScriptBarrierReason::UnmodeledCall => "unmodeled Node call",
        SelectiveJavaScriptBarrierReason::UnsupportedOperation => "unsupported Node operation",
    }
}
