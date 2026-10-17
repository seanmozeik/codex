//! Resolve only the read requested by the materializer's selected lazy branch.

use codex_code_activity as activity;

pub(super) fn binding(
    edit: &activity::PredictedEdit,
    read: activity::SourceReadId,
) -> Option<activity::SnapshotReadBinding> {
    if edit.evidence.source_id != read.source_id {
        return None;
    }
    match &edit.details {
        activity::SelectiveEditDetails::Python { prediction, .. } => {
            let content = match &prediction.operation {
                activity::PythonOperation::WriteText { content, .. } => content,
                activity::PythonOperation::LogicalWrite(write) => &write.content,
                activity::PythonOperation::Truncate => return None,
            };
            python(content, read, 0)
        }
        activity::SelectiveEditDetails::JavaScript { prediction, .. } => {
            javascript(&prediction.content, read, 0)
        }
        _ => None,
    }
}

fn python(
    recipe: &activity::TextRecipe,
    read: activity::SourceReadId,
    depth: usize,
) -> Option<activity::SnapshotReadBinding> {
    if depth >= 64 {
        return None;
    }
    match recipe {
        activity::TextRecipe::Read {
            read_id,
            target,
            evidence,
            filesystem_epoch,
            encoding,
            newline,
        } if *read_id == read.read_id && evidence.source_id == read.source_id => {
            Some(activity::SnapshotReadBinding::new(
                read,
                activity::SnapshotEpoch::new(*filesystem_epoch),
                target.clone(),
                *encoding,
                match newline {
                    None | Some(activity::PythonReadNewline::UniversalTranslate) => {
                        activity::SnapshotReadNewline::PythonUniversal
                    }
                    Some(activity::PythonReadNewline::Preserve) => {
                        activity::SnapshotReadNewline::Preserve
                    }
                    _ => return None,
                },
            ))
        }
        activity::TextRecipe::Replace { input, .. }
        | activity::TextRecipe::SplitItem { input, .. }
        | activity::TextRecipe::Rstrip { input, .. } => python(input, read, depth + 1),
        activity::TextRecipe::ReplaceText {
            input, old, new, ..
        } => python(input, read, depth + 1)
            .or_else(|| python(old, read, depth + 1))
            .or_else(|| python(new, read, depth + 1)),
        activity::TextRecipe::Concat { left, right, .. } => {
            python(left, read, depth + 1).or_else(|| python(right, read, depth + 1))
        }
        activity::TextRecipe::Slice {
            input, start, end, ..
        } => python(input, read, depth + 1)
            .or_else(|| {
                start
                    .as_deref()
                    .and_then(|value| index(value, read, depth + 1))
            })
            .or_else(|| {
                end.as_deref()
                    .and_then(|value| index(value, read, depth + 1))
            }),
        activity::TextRecipe::Conditional {
            predicate: value,
            consequence,
            alternative,
            ..
        } => predicate(value, read, depth + 1)
            .or_else(|| python(consequence, read, depth + 1))
            .or_else(|| python(alternative, read, depth + 1)),
        _ => None,
    }
}

fn predicate(
    recipe: &activity::PredicateRecipe,
    read: activity::SourceReadId,
    depth: usize,
) -> Option<activity::SnapshotReadBinding> {
    if depth >= 64 {
        return None;
    }
    match recipe {
        activity::PredicateRecipe::StartsWith { input, .. }
        | activity::PredicateRecipe::EndsWith { input, .. } => python(input, read, depth + 1),
        activity::PredicateRecipe::TextContains {
            needle, haystack, ..
        } => python(needle, read, depth + 1).or_else(|| python(haystack, read, depth + 1)),
        activity::PredicateRecipe::TextEquals { left, right, .. } => {
            python(left, read, depth + 1).or_else(|| python(right, read, depth + 1))
        }
        activity::PredicateRecipe::And { left, right, .. } => {
            predicate(left, read, depth + 1).or_else(|| predicate(right, read, depth + 1))
        }
    }
}

fn index(
    recipe: &activity::IndexRecipe,
    read: activity::SourceReadId,
    depth: usize,
) -> Option<activity::SnapshotReadBinding> {
    if depth >= 64 {
        return None;
    }
    match recipe {
        activity::IndexRecipe::SubstringIndex { input, start, .. } => {
            python(input, read, depth + 1).or_else(|| {
                start
                    .as_deref()
                    .and_then(|value| index(value, read, depth + 1))
            })
        }
        _ => None,
    }
}

fn javascript(
    recipe: &activity::JavaScriptTextRecipe,
    read: activity::SourceReadId,
    depth: usize,
) -> Option<activity::SnapshotReadBinding> {
    if depth >= 64 {
        return None;
    }
    match recipe {
        activity::JavaScriptTextRecipe::ReadUtf8 {
            read_id,
            target,
            evidence,
            filesystem_epoch,
        } if *read_id == read.read_id && evidence.source_id == read.source_id => {
            Some(activity::SnapshotReadBinding::new(
                read,
                activity::SnapshotEpoch::new(*filesystem_epoch),
                target.clone(),
                activity::TextEncoding::Utf8,
                activity::SnapshotReadNewline::Preserve,
            ))
        }
        activity::JavaScriptTextRecipe::ReplaceFirst { input, .. }
        | activity::JavaScriptTextRecipe::ReplaceAll { input, .. }
        | activity::JavaScriptTextRecipe::Slice { input, .. } => javascript(input, read, depth + 1),
        activity::JavaScriptTextRecipe::Concat { left, right, .. } => {
            javascript(left, read, depth + 1).or_else(|| javascript(right, read, depth + 1))
        }
        _ => None,
    }
}
