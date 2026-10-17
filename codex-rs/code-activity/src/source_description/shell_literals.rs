//! Closed shell literals and exact byte ranges in the original shell source.

use super::DescriptionBudget;
use super::SourceDescriptionFallback;
use crate::SourceId;
use crate::Span;
use tree_sitter::Node;

pub(super) fn literal(
    node: Node<'_>,
    source: &str,
    budget: DescriptionBudget,
) -> Result<String, SourceDescriptionFallback> {
    let mut pending = vec![node];
    while let Some(part) = pending.pop() {
        budget.check()?;
        if !matches!(
            part.kind(),
            "word" | "raw_string" | "string" | "string_content" | "concatenation" | "-"
        ) {
            return Err(SourceDescriptionFallback::UnsupportedSource);
        }
        pending.extend(crate::children(part));
    }
    crate::shell::literals::literal(node, source)
        .ok_or(SourceDescriptionFallback::UnsupportedSource)
}

pub(super) struct Part<'tree> {
    pub(super) node: Node<'tree>,
    pub(super) field: Option<&'static str>,
}

pub(super) fn parts(
    node: Node<'_>,
    budget: DescriptionBudget,
) -> Result<Vec<Part<'_>>, SourceDescriptionFallback> {
    let mut result = Vec::new();
    let mut cursor = node.walk();
    for (index, child) in node.children(&mut cursor).enumerate() {
        budget.check()?;
        if child.kind() != "comment" {
            result.push(Part {
                node: child,
                field: node.field_name_for_child(index as u32),
            });
        }
    }
    Ok(result)
}

pub(super) fn ascii_trivia(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c))
}

pub(super) fn range(
    source: &str,
    start: usize,
    end: usize,
) -> Result<Span, SourceDescriptionFallback> {
    let fail = SourceDescriptionFallback::UnsupportedSource;
    source.get(start..end).ok_or(fail)?;
    let prefix = source.get(..start).ok_or(fail)?;
    let through_end = source.get(..end).ok_or(fail)?;
    Ok(Span {
        source_id: SourceId(0),
        start_byte: start,
        end_byte: end,
        start_line: prefix.bytes().filter(|byte| *byte == b'\n').count() + 1,
        end_line: through_end.bytes().filter(|byte| *byte == b'\n').count() + 1,
        start_column: prefix.rfind('\n').map_or(start + 1, |index| start - index),
    })
}
