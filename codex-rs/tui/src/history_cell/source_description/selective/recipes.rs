//! Bounded recipe display; all strings have passed shared disclosure validation.

use super::encoding_label;
use super::location;
use super::target_label;
use codex_protocol::source_description::SelectiveIndexRecipe;
use codex_protocol::source_description::SelectivePredicateRecipe;
use codex_protocol::source_description::SelectiveTextRecipe;
use ratatui::text::Line;

// Presentation validation already bounds recipe depth and size. A read identity
// includes its source: equal local IDs/epochs in separate children are unrelated.
pub(super) fn recipe_lines(
    recipe: &SelectiveTextRecipe,
    source_id: usize,
    indent: &str,
    lines: &mut Vec<Line<'static>>,
) {
    match recipe {
        SelectiveTextRecipe::Literal { value, evidence } => {
            lines.push(Line::from(format!(
                "{indent}Literal {value:?} · {}",
                location(evidence)
            )));
        }
        SelectiveTextRecipe::Read {
            read_id,
            target,
            evidence,
            filesystem_epoch,
            encoding,
            newline,
        } => {
            lines.push(Line::from(format!("{indent}Read {source_id}:{read_id} at source-local filesystem epoch {filesystem_epoch} · {} · encoding {} · {}", target_label(target), encoding_label(*encoding), location(evidence))));
            if let Some(newline) = newline {
                lines.push(Line::from(format!(
                    "{indent}Source read newline: {}",
                    super::newline::read_label(*newline)
                )));
            }
        }
        SelectiveTextRecipe::Replace {
            input,
            old,
            new,
            count,
            evidence,
        } => {
            let count = count.map_or_else(
                || "all matches".to_owned(),
                |count| format!("at most {count} matches"),
            );
            lines.push(Line::from(format!(
                "{indent}Replace {old:?} with {new:?}, {count} · {}",
                location(evidence)
            )));
            recipe_lines(input, source_id, &format!("{indent}  "), lines);
        }
        SelectiveTextRecipe::Concat {
            left,
            right,
            evidence,
        } => {
            lines.push(Line::from(format!(
                "{indent}Concatenate left then right · {}",
                location(evidence)
            )));
            let indent = format!("{indent}  ");
            recipe_lines(left, source_id, &indent, lines);
            recipe_lines(right, source_id, &indent, lines);
        }
        SelectiveTextRecipe::ReplaceText {
            input,
            old,
            new,
            count,
            evidence,
        } => {
            lines.push(Line::from(format!(
                "{indent}Replace captured search text with captured replacement, {} · {}",
                count.map_or_else(
                    || "all matches".to_owned(),
                    |count| format!("at most {count} matches")
                ),
                location(evidence)
            )));
            for (label, child) in [("Input", input), ("Search", old), ("Replacement", new)] {
                lines.push(Line::from(format!("{indent}  {label}:")));
                recipe_lines(child, source_id, &format!("{indent}    "), lines);
            }
        }
        SelectiveTextRecipe::Slice {
            input,
            start,
            end,
            evidence,
        } => {
            lines.push(Line::from(format!(
                "{indent}Python code-point slice, unit step · {}",
                location(evidence)
            )));
            recipe_lines(input, source_id, &format!("{indent}  "), lines);
            for (label, bound) in [("Start", start), ("End", end)] {
                lines.push(Line::from(format!("{indent}  {label}:")));
                match bound {
                    Some(bound) => index_lines(bound, source_id, &format!("{indent}    "), lines),
                    None => lines.push(Line::from(format!("{indent}    Python default"))),
                }
            }
        }
        SelectiveTextRecipe::SplitItem {
            input,
            separator,
            maxsplit,
            index,
            evidence,
            ..
        } => {
            lines.push(Line::from(format!("{indent}Split on {separator:?}, maxsplit {maxsplit:?}; item {index}, requiring a valid index · {}", location(evidence))));
            recipe_lines(input, source_id, &format!("{indent}  "), lines);
        }
        SelectiveTextRecipe::Rstrip {
            input,
            characters,
            evidence,
        } => {
            lines.push(Line::from(format!(
                "{indent}Strip trailing characters in set {characters:?} · {}",
                location(evidence)
            )));
            recipe_lines(input, source_id, &format!("{indent}  "), lines);
        }
        SelectiveTextRecipe::Conditional {
            predicate,
            consequence,
            alternative,
            evidence,
        } => {
            lines.push(Line::from(format!(
                "{indent}Conditional text; evaluate only the selected branch · {}",
                location(evidence)
            )));
            predicate_lines(predicate, source_id, &format!("{indent}  "), lines);
            for (label, child) in [("If true", consequence), ("If false", alternative)] {
                lines.push(Line::from(format!("{indent}  {label}:")));
                recipe_lines(child, source_id, &format!("{indent}    "), lines);
            }
        }
    }
}

fn index_lines(
    recipe: &SelectiveIndexRecipe,
    source_id: usize,
    indent: &str,
    lines: &mut Vec<Line<'static>>,
) {
    match recipe {
        SelectiveIndexRecipe::Literal { value, evidence } => {
            lines.push(Line::from(format!(
                "{indent}Integer {value} · {}",
                location(evidence)
            )));
        }
        SelectiveIndexRecipe::SubstringIndex {
            input,
            needle,
            start,
            evidence,
        } => {
            lines.push(Line::from(format!(
                "{indent}Python index of {needle:?}, requiring a match · {}",
                location(evidence)
            )));
            recipe_lines(input, source_id, &format!("{indent}  "), lines);
            if let Some(start) = start {
                lines.push(Line::from(format!("{indent}  Search start:")));
                index_lines(start, source_id, &format!("{indent}    "), lines);
            }
        }
    }
}

fn predicate_lines(
    recipe: &SelectivePredicateRecipe,
    source_id: usize,
    indent: &str,
    lines: &mut Vec<Line<'static>>,
) {
    match recipe {
        SelectivePredicateRecipe::TextContains {
            needle,
            haystack,
            evidence,
        } => {
            lines.push(Line::from(format!(
                "{indent}Python text membership; needle then haystack · {}",
                location(evidence)
            )));
            for (label, child) in [("Needle", needle), ("Haystack", haystack)] {
                lines.push(Line::from(format!("{indent}  {label}:")));
                recipe_lines(child, source_id, &format!("{indent}    "), lines);
            }
        }
        SelectivePredicateRecipe::TextEquals {
            left,
            right,
            evidence,
        } => {
            lines.push(Line::from(format!(
                "{indent}Python text equality; left then right · {}",
                location(evidence)
            )));
            for (label, child) in [("Left", left), ("Right", right)] {
                lines.push(Line::from(format!("{indent}  {label}:")));
                recipe_lines(child, source_id, &format!("{indent}    "), lines);
            }
        }
        SelectivePredicateRecipe::And {
            left,
            right,
            evidence,
        } => {
            lines.push(Line::from(format!(
                "{indent}Boolean and; evaluate right only if left is true · {}",
                location(evidence)
            )));
            for (label, child) in [("Left", left), ("Right if left is true", right)] {
                lines.push(Line::from(format!("{indent}  {label}:")));
                predicate_lines(child, source_id, &format!("{indent}    "), lines);
            }
        }
        SelectivePredicateRecipe::StartsWith {
            input,
            prefix: value,
            evidence,
            ..
        }
        | SelectivePredicateRecipe::EndsWith {
            input,
            suffix: value,
            evidence,
            ..
        } => {
            let name = match recipe {
                SelectivePredicateRecipe::StartsWith { .. } => "Starts with",
                _ => "Ends with",
            };
            lines.push(Line::from(format!(
                "{indent}{name} {value:?} · {}",
                location(evidence)
            )));
            recipe_lines(input, source_id, &format!("{indent}  "), lines);
        }
    }
}
