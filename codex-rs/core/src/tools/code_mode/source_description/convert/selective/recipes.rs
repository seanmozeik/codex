//! Bound recursive copies before the shared wire presentation validator.

use super::super::logical;
use super::Unsupported;
use super::activity;
use super::python::encoding;
use super::span;
use super::target;
use super::wire;

pub(super) fn recipe(
    value: &activity::TextRecipe,
    depth: usize,
) -> Result<wire::SelectiveTextRecipe, Unsupported> {
    if depth > 32 {
        return Err(Unsupported);
    }
    Ok(match value {
        activity::TextRecipe::Literal { value, evidence } => wire::SelectiveTextRecipe::Literal {
            value: value.clone(),
            evidence: span(evidence),
        },
        activity::TextRecipe::Read {
            read_id,
            target: value,
            evidence,
            filesystem_epoch,
            encoding: value_encoding,
            newline,
        } => wire::SelectiveTextRecipe::Read {
            read_id: *read_id,
            target: target(value)?,
            evidence: span(evidence),
            filesystem_epoch: *filesystem_epoch,
            encoding: encoding(*value_encoding),
            newline: newline.map(logical::read_newline).transpose()?,
        },
        activity::TextRecipe::Replace {
            input,
            old,
            new,
            count,
            evidence,
        } => wire::SelectiveTextRecipe::Replace {
            input: Box::new(recipe(input, depth + 1)?),
            old: old.clone(),
            new: new.clone(),
            count: *count,
            evidence: span(evidence),
        },
        activity::TextRecipe::ReplaceText {
            input,
            old,
            new,
            count,
            evidence,
        } => wire::SelectiveTextRecipe::ReplaceText {
            input: Box::new(recipe(input, depth + 1)?),
            old: Box::new(recipe(old, depth + 1)?),
            new: Box::new(recipe(new, depth + 1)?),
            count: *count,
            evidence: span(evidence),
        },
        activity::TextRecipe::Slice {
            input,
            start,
            end,
            evidence,
        } => wire::SelectiveTextRecipe::Slice {
            input: Box::new(recipe(input, depth + 1)?),
            start: start
                .as_ref()
                .map(|value| index(value, depth + 1).map(Box::new))
                .transpose()?,
            end: end
                .as_ref()
                .map(|value| index(value, depth + 1).map(Box::new))
                .transpose()?,
            evidence: span(evidence),
        },
        activity::TextRecipe::SplitItem {
            input,
            separator,
            maxsplit,
            index,
            split_evidence,
            evidence,
        } => wire::SelectiveTextRecipe::SplitItem {
            input: Box::new(recipe(input, depth + 1)?),
            separator: separator.clone(),
            maxsplit: *maxsplit,
            index: *index,
            split_evidence: span(split_evidence),
            evidence: span(evidence),
        },
        activity::TextRecipe::Rstrip {
            input,
            characters,
            evidence,
        } => wire::SelectiveTextRecipe::Rstrip {
            input: Box::new(recipe(input, depth + 1)?),
            characters: characters.clone(),
            evidence: span(evidence),
        },
        activity::TextRecipe::Concat {
            left,
            right,
            evidence,
        } => wire::SelectiveTextRecipe::Concat {
            left: Box::new(recipe(left, depth + 1)?),
            right: Box::new(recipe(right, depth + 1)?),
            evidence: span(evidence),
        },
        activity::TextRecipe::Conditional {
            predicate: value,
            consequence,
            alternative,
            evidence,
        } => wire::SelectiveTextRecipe::Conditional {
            predicate: Box::new(predicate(value, depth + 1)?),
            consequence: Box::new(recipe(consequence, depth + 1)?),
            alternative: Box::new(recipe(alternative, depth + 1)?),
            evidence: span(evidence),
        },
    })
}

fn index(
    value: &activity::IndexRecipe,
    depth: usize,
) -> Result<wire::SelectiveIndexRecipe, Unsupported> {
    if depth > 32 {
        return Err(Unsupported);
    }
    Ok(match value {
        activity::IndexRecipe::Literal { value, evidence } => wire::SelectiveIndexRecipe::Literal {
            value: *value,
            evidence: span(evidence),
        },
        activity::IndexRecipe::SubstringIndex {
            input,
            needle,
            start,
            evidence,
        } => wire::SelectiveIndexRecipe::SubstringIndex {
            input: Box::new(recipe(input, depth + 1)?),
            needle: needle.clone(),
            start: start
                .as_ref()
                .map(|value| index(value, depth + 1).map(Box::new))
                .transpose()?,
            evidence: span(evidence),
        },
    })
}

fn predicate(
    value: &activity::PredicateRecipe,
    depth: usize,
) -> Result<wire::SelectivePredicateRecipe, Unsupported> {
    if depth > 32 {
        return Err(Unsupported);
    }
    Ok(match value {
        activity::PredicateRecipe::TextContains {
            needle,
            haystack,
            evidence,
        } => wire::SelectivePredicateRecipe::TextContains {
            needle: Box::new(recipe(needle, depth + 1)?),
            haystack: Box::new(recipe(haystack, depth + 1)?),
            evidence: span(evidence),
        },
        activity::PredicateRecipe::TextEquals {
            left,
            right,
            evidence,
        } => wire::SelectivePredicateRecipe::TextEquals {
            left: Box::new(recipe(left, depth + 1)?),
            right: Box::new(recipe(right, depth + 1)?),
            evidence: span(evidence),
        },
        activity::PredicateRecipe::And {
            left,
            right,
            evidence,
        } => wire::SelectivePredicateRecipe::And {
            left: Box::new(predicate(left, depth + 1)?),
            right: Box::new(predicate(right, depth + 1)?),
            evidence: span(evidence),
        },
        activity::PredicateRecipe::StartsWith {
            input,
            prefix,
            prefix_evidence,
            evidence,
        } => wire::SelectivePredicateRecipe::StartsWith {
            input: Box::new(recipe(input, depth + 1)?),
            prefix: prefix.clone(),
            prefix_evidence: span(prefix_evidence),
            evidence: span(evidence),
        },
        activity::PredicateRecipe::EndsWith {
            input,
            suffix,
            suffix_evidence,
            evidence,
        } => wire::SelectivePredicateRecipe::EndsWith {
            input: Box::new(recipe(input, depth + 1)?),
            suffix: suffix.clone(),
            suffix_evidence: span(suffix_evidence),
            evidence: span(evidence),
        },
    })
}
