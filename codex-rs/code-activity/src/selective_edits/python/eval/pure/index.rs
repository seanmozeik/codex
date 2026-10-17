//! Lazy integer bounds, including captured standard substring-index calls.

use super::Builder;
use super::PureError;
use super::Result;
use crate::SourceDescriptionFallback as Fallback;
use crate::selective_edits::python::IndexRecipe;
use crate::selective_edits::python::PythonAssumption;
use crate::selective_edits::python::PythonDependency;
use crate::selective_edits::python::eval::Kind;
use crate::selective_edits::python::eval::MAX_RECIPE_DEPTH;
use crate::selective_edits::python::eval::Method;
use crate::selective_edits::python::eval::strings::IndexValue;
use crate::selective_edits::python::literal;
use crate::selective_edits::python::syntax::field;
use crate::selective_edits::python::syntax::parts;
use crate::selective_edits::python::syntax::unparenthesized;
use tree_sitter::Node;

impl Builder<'_, '_> {
    pub(super) fn index_bound(
        &mut self,
        node: Node<'_>,
        depth: usize,
    ) -> Result<(IndexValue, Vec<PythonDependency>)> {
        if depth >= MAX_RECIPE_DEPTH {
            return Err(Fallback::SourceLimit.into());
        }
        self.guard.charge(128)?;
        let node = unparenthesized(node)?;
        let (integer, dependencies) = match node.kind() {
            "integer" => (
                literal::integer(crate::text(node, self.source))?,
                Vec::new(),
            ),
            "unary_operator" => {
                let value = literal::integer(crate::text(
                    unparenthesized(field(node, "argument")?)?,
                    self.source,
                ))?;
                let value = match crate::text(field(node, "operator")?, self.source) {
                    "+" => value,
                    "-" => value.checked_neg().ok_or(Fallback::UnsupportedSource)?,
                    _ => return Err(PureError::Unsupported),
                };
                (value, Vec::new())
            }
            "identifier" => {
                let value = self.lookup(node)?;
                match value.kind {
                    Kind::Index(index) => return Ok((index, value.dependencies)),
                    Kind::Integer(Some(integer)) => (integer, value.dependencies),
                    _ => return Err(PureError::Unsupported),
                }
            }
            "call" => return self.substring_index(node, depth + 1),
            _ => return Err(PureError::Unsupported),
        };
        Ok((
            IndexValue {
                recipe: IndexRecipe::Literal {
                    value: integer,
                    evidence: crate::span(node, self.source_id),
                },
                depth: 1,
                weight: 128,
            },
            dependencies,
        ))
    }

    fn substring_index(
        &mut self,
        node: Node<'_>,
        depth: usize,
    ) -> Result<(IndexValue, Vec<PythonDependency>)> {
        let (input, mut dependencies) =
            self.receiver(field(node, "function")?, Method::Index, depth)?;
        let arguments: Vec<_> = parts(field(node, "arguments")?).take(3).collect();
        if !(1..=2).contains(&arguments.len())
            || arguments
                .iter()
                .any(|argument| argument.kind() == "keyword_argument")
        {
            return Err(PureError::Unsupported);
        }
        let needle = self.text(arguments[0], depth)?;
        let Some(needle_text) = needle.literal() else {
            return Err(PureError::Unsupported);
        };
        self.guard.charge(needle_text.len() + 128)?;
        let needle_text = needle_text.to_owned();
        self.merge(&mut dependencies, needle.dependencies)?;
        let start = if arguments.len() == 2 {
            let (start, start_dependencies) = self.index_bound(arguments[1], depth)?;
            self.merge(&mut dependencies, start_dependencies)?;
            Some(start)
        } else {
            None
        };
        let recipe_depth = input
            .depth
            .max(start.as_ref().map_or(0, |value| value.depth))
            + 1;
        if recipe_depth > MAX_RECIPE_DEPTH {
            return Err(Fallback::SourceLimit.into());
        }
        self.dependency(
            &mut dependencies,
            PythonDependency::Capability {
                assumption: PythonAssumption::StandardTextMethods,
            },
        )?;
        let weight =
            input.weight + needle_text.len() + start.as_ref().map_or(0, |value| value.weight) + 128;
        // Do not pre-evaluate even a literal missing needle: this call may live
        // in the unselected arm. The recipe carries its selected-only failure.
        Ok((
            IndexValue {
                recipe: IndexRecipe::SubstringIndex {
                    input: Box::new(input.recipe),
                    needle: needle_text,
                    start: start.map(|value| Box::new(value.recipe)),
                    evidence: crate::span(node, self.source_id),
                },
                depth: recipe_depth,
                weight,
            },
            dependencies,
        ))
    }
}
