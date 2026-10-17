//! Synchronous single-owner context shapes, checked before source evaluation.

use super::Checker;
use super::field;
use super::only;
use super::parts;
use crate::SourceDescriptionFallback as Fallback;
use crate::selective_edits::python::literal;
use tree_sitter::Node;

pub(crate) struct Shape<'t> {
    pub(crate) manager: Node<'t>,
    pub(crate) owner: Node<'t>,
    pub(crate) binding: Node<'t>,
    pub(crate) body: Node<'t>,
}

pub(crate) fn shape<'t>(node: Node<'t>, source: &str) -> Result<Shape<'t>, Fallback> {
    let mut cursor = node.walk();
    if parts(node).count() != 2
        || node
            .children(&mut cursor)
            .any(|child| child.kind() == "async")
    {
        return Err(Fallback::UnsupportedSource);
    }
    let clause = parts(node).next().ok_or(Fallback::UnsupportedSource)?;
    if clause.kind() != "with_clause" {
        return Err(Fallback::UnsupportedSource);
    }
    // The pinned grammar accepts an unparenthesized trailing comma, although
    // Python requires parentheses for that spelling.
    let mut cursor = clause.walk();
    if clause
        .children(&mut cursor)
        .filter(|child| !matches!(child.kind(), "comment" | "line_continuation"))
        .last()
        .is_some_and(|child| child.kind() == ",")
    {
        return Err(Fallback::UnsupportedSource);
    }
    let item = only(clause)?;
    if item.kind() != "with_item" {
        return Err(Fallback::UnsupportedSource);
    }
    let mut binding = field(item, "value")?;
    if matches!(binding.kind(), "parenthesized_expression" | "tuple") {
        // The grammar can give the one context-list pair to an expression
        // wrapper. Do not also unwrap an inner pair from a parenthesized clause.
        let mut cursor = clause.walk();
        if clause
            .children(&mut cursor)
            .any(|child| matches!(child.kind(), "(" | ")"))
        {
            return Err(Fallback::UnsupportedSource);
        }
        binding = only(binding)?;
    }
    if binding.kind() != "as_pattern" || parts(binding).count() != 2 {
        return Err(Fallback::UnsupportedSource);
    }
    let target = field(binding, "alias")?;
    if target.kind() != "as_pattern_target" {
        return Err(Fallback::UnsupportedSource);
    }
    // The pinned grammar aliases expression to a named wrapper, whose one
    // identifier child is the owner. Destructuring and other targets stay out.
    let owner = only(target)?;
    if owner.kind() != "identifier" || !literal::identifier(crate::text(owner, source)) {
        return Err(Fallback::UnsupportedSource);
    }
    Ok(Shape {
        manager: parts(binding).next().ok_or(Fallback::UnsupportedSource)?,
        owner,
        binding,
        body: field(node, "body")?,
    })
}

impl Checker<'_> {
    pub(super) fn context(&self, node: Node<'_>) -> Result<(), Fallback> {
        let shape = shape(node, self.source)?;
        self.expression(shape.manager)?;
        if shape.body.kind() != "block" {
            return Err(Fallback::UnsupportedSource);
        }
        let first = parts(shape.body)
            .next()
            .ok_or(Fallback::UnsupportedSource)?;
        let inline = first.start_position().row == node.start_position().row;
        let indentation = first.start_position().column;
        if !inline && indentation <= node.start_position().column {
            return Err(Fallback::UnsupportedSource);
        }
        for statement in parts(shape.body) {
            if inline && statement.start_position().row != first.start_position().row {
                return Err(Fallback::UnsupportedSource);
            }
            self.indentation(statement, indentation)?;
            self.statement(statement, false)?;
        }
        Ok(())
    }
}
