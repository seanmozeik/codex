//! Closed replacement calls, with no access to ordered execution state.

use super::Builder;
use super::PureError;
use super::Result;
use crate::selective_edits::python::PythonDependency;
use crate::selective_edits::python::TextRecipe;
use crate::selective_edits::python::eval::Kind;
use crate::selective_edits::python::eval::Method;
use crate::selective_edits::python::eval::Value;
use crate::selective_edits::python::literal;
use crate::selective_edits::python::syntax::field;
use crate::selective_edits::python::syntax::parts;
use crate::selective_edits::python::syntax::unparenthesized;
use tree_sitter::Node;

impl Builder<'_, '_> {
    pub(super) fn replace(&mut self, node: Node<'_>, depth: usize) -> Result<Value> {
        let (input, mut dependencies) =
            self.receiver(field(node, "function")?, Method::Replace, depth)?;
        // Retain at most one extra argument to reject the closed signature.
        let arguments: Vec<_> = parts(field(node, "arguments")?).take(4).collect();
        if !(2..=3).contains(&arguments.len())
            || arguments
                .iter()
                .any(|node| node.kind() == "keyword_argument")
        {
            return Err(PureError::Unsupported);
        }
        let old = self.text(arguments[0], depth)?;
        let new = self.text(arguments[1], depth)?;
        self.merge(&mut dependencies, old.dependencies)?;
        self.merge(&mut dependencies, new.dependencies)?;
        let count = if arguments.len() == 3 {
            let (value, count_dependencies) = self.count(arguments[2])?;
            self.merge(&mut dependencies, count_dependencies)?;
            Some(value)
        } else {
            None
        };
        let (Kind::Text(old), Kind::Text(new)) = (old.kind, new.kind) else {
            return Err(PureError::Unsupported);
        };
        let recipe = TextRecipe::ReplaceText {
            input: Box::new(input.recipe),
            old: Box::new(old.recipe),
            new: Box::new(new.recipe),
            count,
            evidence: crate::span(node, self.source_id),
        };
        self.finish(
            recipe,
            input.depth.max(old.depth).max(new.depth) + 1,
            input.weight + old.weight + new.weight + 128,
            dependencies,
        )
    }

    fn count(&mut self, node: Node<'_>) -> Result<(usize, Vec<PythonDependency>)> {
        let node = unparenthesized(node)?;
        let (value, dependencies) = match node.kind() {
            "integer" => (
                literal::integer(crate::text(node, self.source))?,
                Vec::new(),
            ),
            "unary_operator" => {
                let value = literal::integer(crate::text(
                    unparenthesized(field(node, "argument")?)?,
                    self.source,
                ))?;
                if crate::text(field(node, "operator")?, self.source) != "+" {
                    return Err(PureError::Unsupported);
                }
                (value, Vec::new())
            }
            "identifier" => {
                let value = self.lookup(node)?;
                let Kind::Integer(Some(integer)) = value.kind else {
                    return Err(PureError::Unsupported);
                };
                (integer, value.dependencies)
            }
            _ => return Err(PureError::Unsupported),
        };
        let value = usize::try_from(value).map_err(|_| PureError::Unsupported)?;
        Ok((value, dependencies))
    }
}
