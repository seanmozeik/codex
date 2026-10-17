//! Logical forecasts retain current acceptance and future realization premises.

use super::encoding_label;
use super::location;
use super::newline::write_label;
use super::recipes::recipe_lines;
use codex_protocol::source_description::SelectiveLogicalStreamRealization;
use codex_protocol::source_description::SelectivePythonLogicalWrite;
use codex_protocol::source_description::SelectivePythonStreamMode;
use codex_protocol::source_description::SelectivePythonStreamPosition;
use codex_protocol::source_description::SelectivePythonWriteResultCondition;
use codex_protocol::source_description::SelectiveSnapshotPreview;
use ratatui::style::Stylize as _;
use ratatui::text::Line;

pub(super) fn operation(
    write: &SelectivePythonLogicalWrite,
    source_id: usize,
    lines: &mut Vec<Line<'static>>,
) {
    lines.push(Line::from(format!(
        "Operation: conditional logical stream write · encoding {} · output newline {}",
        encoding_label(write.encoding),
        write_label(write.newline)
    )));
    lines.push(Line::from(format!(
        "Stream {source_id}:{} · opened at {} · revision {} · physical seed epoch {}",
        write.stream_id.get(),
        location(&write.opened_at),
        write.stream_revision,
        write.base_filesystem_epoch
    )));
    lines.push(Line::from(match write.mode {
        SelectivePythonStreamMode::TruncateWrite => {
            "Mode: w · requires an empty post-truncation seed"
        }
        SelectivePythonStreamMode::UpdateExisting => {
            "Mode: r+ · retain the untouched encoded-byte suffix"
        }
    }));
    match &write.position {
        SelectivePythonStreamPosition::InitialZero => {
            lines.push(Line::from("Position: initial zero"));
        }
        SelectivePythonStreamPosition::RewoundZero {
            read_id,
            read_evidence,
            seek_evidence,
        } => {
            lines.push(Line::from(format!(
                "Position: zero after full read {source_id}:{read_id} · {} · seek(0) at {}",
                location(read_evidence),
                location(seek_evidence)
            )));
        }
    }
    lines.push(
        Line::from(match write.required_result {
            SelectivePythonWriteResultCondition::FullCharacterCount => {
                "Requires write to accept the full character count before newline translation."
            }
        })
        .dim(),
    );
    lines.push(Line::from("Physical realization requires a later successful flush or close; current and final file bytes are unobserved.").dim());
    recipe_lines(&write.content, source_id, "  ", lines);
}

pub(super) fn snapshot_conditions(
    snapshot: &SelectiveSnapshotPreview,
    indent: &str,
    lines: &mut Vec<Line<'static>>,
) {
    let Some(logical) = &snapshot.logical_stream else {
        return;
    };
    lines.push(
        Line::from(format!(
            "{indent}Logical stream {}:{} · revision {} → {} · physical seed epoch {}",
            logical.binding.source_id,
            logical.binding.stream_id.get(),
            logical.binding.revision,
            logical.revision_after,
            logical.binding.base_filesystem_epoch
        ))
        .dim(),
    );
    lines.push(Line::from(format!(
        "{indent}Requires full character count: {} accepted before newline translation; {} encoded payload bytes.",
        logical.accepted_characters_required,
        logical.payload_bytes.len()
    )).dim());
    let realization = match logical.realization {
        SelectiveLogicalStreamRealization::RequiresSuccessfulFlushOrClose => {
            "Physical realization requires a later successful flush or close."
        }
    };
    lines.push(Line::from(format!("{indent}{realization}")).dim());
    lines.push(
        Line::from(format!(
            "{indent}Conditional logical content; current and final file bytes are unobserved."
        ))
        .dim(),
    );
}

#[cfg(test)]
mod tests;
