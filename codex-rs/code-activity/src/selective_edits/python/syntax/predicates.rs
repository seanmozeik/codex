//! Exact finite comparison/conjunction shapes shared by validation and capture.

use super::field;
use super::parts;
use crate::SourceDescriptionFallback as Fallback;
use tree_sitter::Node;

pub(in crate::selective_edits::python) enum TextComparison {
    Contains,
    Equals,
}

pub(in crate::selective_edits::python) fn comparison_parts<'t>(
    node: Node<'t>,
    source: &str,
) -> Result<(TextComparison, Node<'t>, Node<'t>), Fallback> {
    if node.kind() != "comparison_operator" {
        return Err(Fallback::UnsupportedSource);
    }
    let mut operands = parts(node);
    let left = operands.next().ok_or(Fallback::UnsupportedSource)?;
    let right = operands.next().ok_or(Fallback::UnsupportedSource)?;
    if operands.next().is_some() {
        return Err(Fallback::UnsupportedSource);
    }
    let mut cursor = node.walk();
    let mut operators = node.children_by_field_name("operators", &mut cursor);
    let operator = operators.next().ok_or(Fallback::UnsupportedSource)?;
    if operators.next().is_some() {
        return Err(Fallback::UnsupportedSource);
    }
    let comparison = match crate::text(operator, source) {
        "in" => TextComparison::Contains,
        "==" => TextComparison::Equals,
        _ => return Err(Fallback::UnsupportedSource),
    };
    Ok((comparison, left, right))
}

pub(in crate::selective_edits::python) fn conjunction_parts<'t>(
    node: Node<'t>,
    source: &str,
) -> Result<(Node<'t>, Node<'t>), Fallback> {
    if node.kind() != "boolean_operator"
        || parts(node).count() != 2
        || crate::text(field(node, "operator")?, source) != "and"
    {
        return Err(Fallback::UnsupportedSource);
    }
    Ok((field(node, "left")?, field(node, "right")?))
}

/// Validation admits both lazy operators, but capture remains a closed registry.
pub(in crate::selective_edits::python) fn boolean_parts<'t>(
    node: Node<'t>,
    source: &str,
) -> Result<(Node<'t>, Node<'t>), Fallback> {
    if node.kind() != "boolean_operator"
        || parts(node).count() != 2
        || !matches!(crate::text(field(node, "operator")?, source), "and" | "or")
    {
        return Err(Fallback::UnsupportedSource);
    }
    Ok((field(node, "left")?, field(node, "right")?))
}
