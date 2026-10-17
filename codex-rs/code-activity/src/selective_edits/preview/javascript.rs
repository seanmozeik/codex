//! Node recipes preserve replacement modes, UTF-16 slice indices and UTF-8 newlines.

use super::SnapshotEpoch;
use super::SnapshotPreviewError;
use super::SnapshotReadNewline;
use super::SourceReadId;
use super::budget::Budget;
use super::snapshots::ReadExpectation;
use super::snapshots::Snapshots;
use super::text;
use crate::selective_edits::JavaScriptEdit;
use crate::selective_edits::JavaScriptTextRecipe;
use crate::selective_edits::JavaScriptWriteEffect;
use crate::selective_edits::TextEncoding;

pub(super) fn materialize(
    edit: &JavaScriptEdit,
    before: Option<&str>,
    snapshots: &mut Snapshots<'_>,
    budget: &mut Budget,
) -> Result<String, SnapshotPreviewError> {
    match &edit.write_effect {
        JavaScriptWriteEffect::Overwrite => evaluate(&edit.content, snapshots, budget, 1),
        JavaScriptWriteEffect::Truncate { .. } => Ok(String::new()),
        JavaScriptWriteEffect::DescriptorAppend { .. } => {
            let before = before.ok_or(SnapshotPreviewError::MissingDescriptorPreimage)?;
            let content = evaluate(&edit.content, snapshots, budget, 1)?;
            text::concat(before, &content, budget)
        }
        JavaScriptWriteEffect::DescriptorWrite { byte_offset, .. } => {
            let before = before.ok_or(SnapshotPreviewError::MissingDescriptorPreimage)?;
            let content = evaluate(&edit.content, snapshots, budget, 1)?;
            if content.is_empty() {
                return budget.copy(before);
            }
            let end = byte_offset.saturating_add(content.len());
            let size = end.max(before.len());
            budget.output(size)?;
            let mut bytes = Vec::with_capacity(size);
            bytes.extend_from_slice(before.as_bytes());
            bytes.resize(size, 0);
            bytes[*byte_offset..end].copy_from_slice(content.as_bytes());
            String::from_utf8(bytes).map_err(|_| SnapshotPreviewError::InvalidUtf8)
        }
    }
}

fn evaluate(
    recipe: &JavaScriptTextRecipe,
    snapshots: &mut Snapshots<'_>,
    budget: &mut Budget,
    depth: usize,
) -> Result<String, SnapshotPreviewError> {
    budget.enter(depth)?;
    match recipe {
        JavaScriptTextRecipe::Literal { value, .. } => budget.copy(value),
        JavaScriptTextRecipe::ReadUtf8 {
            read_id,
            target,
            evidence,
            filesystem_epoch,
        } => {
            let binding = ReadExpectation {
                read: SourceReadId::new(evidence.source_id, *read_id),
                filesystem_epoch: SnapshotEpoch::new(*filesystem_epoch),
                target,
                encoding: TextEncoding::Utf8,
                newline: SnapshotReadNewline::Preserve,
            };
            snapshots.read(binding, budget)
        }
        JavaScriptTextRecipe::ReplaceFirst {
            input, old, new, ..
        } => {
            if new.contains('$') {
                return Err(SnapshotPreviewError::UnsupportedOperation);
            }
            let input = evaluate(input, snapshots, budget, depth + 1)?;
            text::replace(&input, old, new, Some(1), budget)
        }
        JavaScriptTextRecipe::ReplaceAll {
            input, old, new, ..
        } => {
            if old.is_empty() || new.contains('$') {
                return Err(SnapshotPreviewError::UnsupportedOperation);
            }
            let input = evaluate(input, snapshots, budget, depth + 1)?;
            text::replace(&input, old, new, None, budget)
        }
        JavaScriptTextRecipe::Slice {
            input, start, end, ..
        } => {
            let input = evaluate(input, snapshots, budget, depth + 1)?;
            slice(&input, *start, *end, budget)
        }
        JavaScriptTextRecipe::Concat { left, right, .. } => {
            let left = evaluate(left, snapshots, budget, depth + 1)?;
            let right = evaluate(right, snapshots, budget, depth + 1)?;
            text::concat(&left, &right, budget)
        }
    }
}

fn slice(
    input: &str,
    start: usize,
    end: Option<usize>,
    budget: &mut Budget,
) -> Result<String, SnapshotPreviewError> {
    budget.work(input.len().saturating_mul(3))?;
    let length = input.chars().map(char::len_utf16).sum::<usize>();
    let start = start.min(length);
    let end = end.unwrap_or(length).min(length);
    if end <= start {
        return budget.copy("");
    }
    let start =
        utf16_byte_offset(input, start).ok_or(SnapshotPreviewError::UnsupportedOperation)?;
    let end = utf16_byte_offset(input, end).ok_or(SnapshotPreviewError::UnsupportedOperation)?;
    budget.copy(&input[start..end])
}

fn utf16_byte_offset(input: &str, index: usize) -> Option<usize> {
    let mut units = 0;
    for (offset, character) in input.char_indices() {
        if units == index {
            return Some(offset);
        }
        units += character.len_utf16();
        if units > index {
            return None;
        }
    }
    (units == index).then_some(input.len())
}
