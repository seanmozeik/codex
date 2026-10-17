//! Small useful operation summaries; full provenance remains in disclosure.

use super::SelectiveCell;
use codex_protocol::source_description::SelectiveEditDetails;
use codex_protocol::source_description::SelectiveJavaScriptTextRecipe;
use codex_protocol::source_description::SelectiveJavaScriptWriteEffect;
use codex_protocol::source_description::SelectivePythonOperation;
use codex_protocol::source_description::SelectiveTextRecipe;
use ratatui::style::Stylize as _;
use ratatui::text::Line;

const VISIBLE_OPERATIONS: usize = 3;

pub(super) fn compact(cell: &SelectiveCell) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(cell.summary()).dim()];
    for edit in cell.edits.iter().take(VISIBLE_OPERATIONS) {
        let text = match &edit.details {
            SelectiveEditDetails::Python { prediction, .. } => {
                let action = match &prediction.operation {
                    SelectivePythonOperation::Truncate => "truncate".to_owned(),
                    SelectivePythonOperation::WriteText {
                        content, newline, ..
                    } => {
                        let action = python(content);
                        match newline {
                            Some(newline) => format!(
                                "{action} · output newline {}",
                                super::newline::write_label(*newline)
                            ),
                            None => action,
                        }
                    }
                    SelectivePythonOperation::LogicalWrite(write) => format!(
                        "conditional logical stream · {} · output newline {} · requires full character count and later successful flush or close",
                        python(&write.content),
                        super::newline::write_label(write.newline)
                    ),
                };
                format!("{} · {action}", snippet(&prediction.target.path))
            }
            SelectiveEditDetails::JavaScript { prediction, .. } => {
                let action = match &prediction.write_effect {
                    SelectiveJavaScriptWriteEffect::Overwrite => javascript(&prediction.content),
                    SelectiveJavaScriptWriteEffect::Truncate { .. } => {
                        "truncate at descriptor open".to_owned()
                    }
                    SelectiveJavaScriptWriteEffect::DescriptorAppend { .. } => {
                        "conditionally append UTF-8 at operation-time EOF".to_owned()
                    }
                    SelectiveJavaScriptWriteEffect::DescriptorWrite { byte_offset, .. } => {
                        format!("write UTF-8 at byte {byte_offset}; preserve suffix")
                    }
                };
                format!("{} · {action}", snippet(&prediction.target.path))
            }
            SelectiveEditDetails::Patch { patch, .. } => {
                format!("Submitted patch · {} file directives", patch.files.len())
            }
        };
        lines.push(Line::from(text));
        if let Some(preview) = &edit.snapshot {
            super::snapshot::compact(preview, edit.later_context, &mut lines);
        }
    }
    if cell.edits.len() > VISIBLE_OPERATIONS {
        lines.push(
            Line::from(format!(
                "… {} more predicted operations",
                cell.edits.len() - VISIBLE_OPERATIONS
            ))
            .dim(),
        );
    }
    lines
}

fn python(recipe: &SelectiveTextRecipe) -> String {
    match recipe {
        SelectiveTextRecipe::Literal { value, .. } => format!("write {}", snippet(value)),
        SelectiveTextRecipe::Read {
            target, newline, ..
        } => {
            let action = format!("write text read from {}", snippet(&target.path));
            match newline {
                Some(newline) => format!(
                    "{action} · read newline {}",
                    super::newline::read_label(*newline)
                ),
                None => action,
            }
        }
        SelectiveTextRecipe::Replace {
            old, new, count, ..
        } => format!(
            "replace {} with {} ({})",
            snippet(old),
            snippet(new),
            count.map_or_else(
                || "all matches".to_owned(),
                |count| format!("up to {count}")
            )
        ),
        SelectiveTextRecipe::ReplaceText { .. } => "replace captured text".to_owned(),
        SelectiveTextRecipe::Slice { .. } => "write a Python code-point slice".to_owned(),
        SelectiveTextRecipe::SplitItem {
            separator, index, ..
        } => format!("write split item {index} using {}", snippet(separator)),
        SelectiveTextRecipe::Rstrip { characters, .. } => {
            format!("strip trailing characters in {}", snippet(characters))
        }
        SelectiveTextRecipe::Concat { .. } => "write concatenated text".to_owned(),
        SelectiveTextRecipe::Conditional { .. } => {
            "write conditional text; branch unresolved".to_owned()
        }
    }
}

fn javascript(recipe: &SelectiveJavaScriptTextRecipe) -> String {
    match recipe {
        SelectiveJavaScriptTextRecipe::Literal { value, .. } => {
            format!("Node write {}", snippet(value))
        }
        SelectiveJavaScriptTextRecipe::ReadUtf8 { target, .. } => {
            format!("Node write UTF-8 text read from {}", snippet(&target.path))
        }
        SelectiveJavaScriptTextRecipe::ReplaceFirst { old, new, .. } => {
            format!("replace first {} with {}", snippet(old), snippet(new))
        }
        SelectiveJavaScriptTextRecipe::ReplaceAll { old, new, .. } => {
            format!("replace all {} with {}", snippet(old), snippet(new))
        }
        SelectiveJavaScriptTextRecipe::Slice { .. } => "Node write a UTF-16 slice".to_owned(),
        SelectiveJavaScriptTextRecipe::Concat { .. } => "Node write concatenated text".to_owned(),
    }
}

fn snippet(value: &str) -> String {
    let mut characters = value.chars();
    let prefix = characters.by_ref().take(60).collect::<String>();
    if characters.next().is_some() {
        format!("{prefix:?}…")
    } else {
        format!("{prefix:?}")
    }
}
