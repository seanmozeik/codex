//! Unit-step text slices over closed receivers and lazy index bounds.

use super::Builder;
use super::PureError;
use super::Result;
use crate::selective_edits::python::TextRecipe;
use crate::selective_edits::python::eval::Kind;
use crate::selective_edits::python::eval::Value;
use crate::selective_edits::python::syntax::field;
use crate::selective_edits::python::syntax::parts;
use tree_sitter::Node;

impl Builder<'_, '_> {
    pub(super) fn slice(&mut self, node: Node<'_>, depth: usize) -> Result<Value> {
        let input = self.text(field(node, "value")?, depth)?;
        let Kind::Text(input_text) = input.kind else {
            return Err(PureError::Unsupported);
        };
        let subscript = field(node, "subscript")?;
        if subscript.kind() != "slice" {
            return Err(PureError::Unsupported);
        }
        // The pinned grammar names neither bound. Locate the actual colon,
        // which cannot be confused with a colon inside a string literal.
        let mut cursor = subscript.walk();
        let mut colons = subscript
            .children(&mut cursor)
            .filter(|node| node.kind() == ":");
        let colon = colons.next().ok_or(PureError::Unsupported)?;
        if colons.next().is_some() {
            return Err(PureError::Unsupported);
        }
        let mut dependencies = input.dependencies;
        let mut start = None;
        let mut end = None;
        for (index, bound) in parts(subscript).enumerate() {
            if index >= 2 {
                return Err(PureError::Unsupported);
            }
            let (value, bound_dependencies) = self.index_bound(bound, depth)?;
            self.merge(&mut dependencies, bound_dependencies)?;
            if bound.end_byte() <= colon.start_byte() {
                start = Some(value);
            } else {
                end = Some(value);
            }
        }
        let recipe_depth = input_text
            .depth
            .max(start.as_ref().map_or(0, |value| value.depth))
            .max(end.as_ref().map_or(0, |value| value.depth))
            + 1;
        let weight = input_text.weight
            + start.as_ref().map_or(0, |value| value.weight)
            + end.as_ref().map_or(0, |value| value.weight)
            + 128;
        self.finish(
            TextRecipe::Slice {
                input: Box::new(input_text.recipe),
                start: start.map(|value| Box::new(value.recipe)),
                end: end.map(|value| Box::new(value.recipe)),
                evidence: crate::span(node, self.source_id),
            },
            recipe_depth,
            weight,
            dependencies,
        )
    }
}
