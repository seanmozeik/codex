//! Bounded immutable text primitives with Python Unicode-code-point indices.

use super::SnapshotPreviewError;
use super::budget::Budget;
use crate::Span;

pub(super) fn replace(
    input: &str,
    old: &str,
    new: &str,
    count: Option<usize>,
    budget: &mut Budget,
) -> Result<String, SnapshotPreviewError> {
    budget.work(
        input
            .len()
            .saturating_add(old.len())
            .saturating_add(new.len()),
    )?;
    let limit = count.unwrap_or(usize::MAX);
    let matches = input.match_indices(old).take(limit).count();
    let removed = matches.saturating_mul(old.len());
    let inserted = matches.saturating_mul(new.len());
    let length = input.len().saturating_sub(removed).saturating_add(inserted);
    budget.output(length)?;
    budget.work(input.len().saturating_add(old.len()))?;
    let mut output = String::with_capacity(length);
    let mut position = 0;
    // Rust's empty-string matches are Unicode scalar boundaries, matching the
    // Python scalar subset represented by these UTF-8 recipes.
    for (offset, _) in input.match_indices(old).take(limit) {
        output.push_str(&input[position..offset]);
        output.push_str(new);
        position = offset + old.len();
    }
    output.push_str(&input[position..]);
    Ok(output)
}

pub(super) fn concat(
    left: &str,
    right: &str,
    budget: &mut Budget,
) -> Result<String, SnapshotPreviewError> {
    let length = left.len().saturating_add(right.len());
    budget.output(length)?;
    let mut output = String::with_capacity(length);
    output.push_str(left);
    output.push_str(right);
    Ok(output)
}

pub(super) fn slice(
    input: &str,
    start: Option<i64>,
    end: Option<i64>,
    budget: &mut Budget,
) -> Result<String, SnapshotPreviewError> {
    budget.work(input.len().saturating_mul(3))?;
    let length = input.chars().count();
    let start = start.map_or(0, |value| bound(value, length));
    let end = end.map_or(length, |value| bound(value, length)).max(start);
    let start = byte_offset(input, start);
    let end = byte_offset(input, end);
    budget.copy(&input[start..end])
}

pub(super) fn index(
    input: &str,
    needle: &str,
    start: Option<i64>,
    evidence: &Span,
    budget: &mut Budget,
) -> Result<i64, SnapshotPreviewError> {
    budget.work(input.len().saturating_mul(4).saturating_add(needle.len()))?;
    let length = input.chars().count();
    let start = start.unwrap_or(0);
    // Unlike slicing, a positive start beyond len must fail even for "".
    if start >= 0 && usize::try_from(start).map_or(true, |value| value > length) {
        return Err(SnapshotPreviewError::SubstringNotFound {
            evidence: evidence.clone(),
        });
    }
    let start = bound(start, length);
    let byte_start = byte_offset(input, start);
    let relative = input[byte_start..].find(needle).ok_or_else(|| {
        SnapshotPreviewError::SubstringNotFound {
            evidence: evidence.clone(),
        }
    })?;
    let index = start + input[byte_start..byte_start + relative].chars().count();
    i64::try_from(index).map_err(|_| SnapshotPreviewError::UnsupportedOperation)
}

pub(super) fn split_item(
    input: &str,
    separator: &str,
    maxsplit: Option<i64>,
    index: i64,
    evidence: &Span,
    budget: &mut Budget,
) -> Result<String, SnapshotPreviewError> {
    if separator.is_empty() {
        return Err(SnapshotPreviewError::EmptySeparator);
    }
    budget.work(
        input
            .len()
            .saturating_mul(2)
            .saturating_add(separator.len()),
    )?;
    let maximum = maxsplit
        .filter(|value| *value >= 0)
        .map_or(usize::MAX, |value| {
            usize::try_from(value)
                .unwrap_or(usize::MAX)
                .saturating_add(1)
        });
    let count = input.splitn(maximum, separator).count();
    let selected = if index >= 0 {
        usize::try_from(index).ok().filter(|value| *value < count)
    } else {
        usize::try_from(index.unsigned_abs())
            .ok()
            .and_then(|value| count.checked_sub(value))
    };
    let item = selected
        .and_then(|selected| input.splitn(maximum, separator).nth(selected))
        .ok_or_else(|| SnapshotPreviewError::SplitIndexOutOfRange {
            evidence: evidence.clone(),
        })?;
    budget.copy(item)
}

pub(super) fn rstrip(
    input: &str,
    characters: &str,
    budget: &mut Budget,
) -> Result<String, SnapshotPreviewError> {
    budget.work(input.len().saturating_mul(characters.len().max(1)))?;
    budget.copy(input.trim_end_matches(|character: char| characters.contains(character)))
}

fn bound(index: i64, length: usize) -> usize {
    if index >= 0 {
        usize::try_from(index).unwrap_or(usize::MAX).min(length)
    } else {
        length.saturating_sub(usize::try_from(index.unsigned_abs()).unwrap_or(usize::MAX))
    }
}

fn byte_offset(text: &str, codepoints: usize) -> usize {
    text.char_indices()
        .nth(codepoints)
        .map_or(text.len(), |(offset, _)| offset)
}
