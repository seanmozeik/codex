//! Direct finite joins lower to existing recipes without retaining iterables.

use super::Kind;
use super::MAX_RECIPE_DEPTH;
use super::MAX_RECORDS;
use super::Result;
use super::State;
use super::TextValue;
use super::Value;
use super::pure;
use super::pure::PureError;
use crate::SourceDescriptionFallback as Fallback;
use crate::selective_edits::python::PythonAssumption;
use crate::selective_edits::python::PythonBarrierReason;
use crate::selective_edits::python::PythonDependency;
use crate::selective_edits::python::TextRecipe;
use crate::selective_edits::python::syntax::field;
use crate::selective_edits::python::syntax::parts;
use crate::selective_edits::python::syntax::unparenthesized;
use tree_sitter::Node;

impl State<'_> {
    pub(super) fn join(&mut self, node: Node<'_>, function: Value) -> Result<Value> {
        let Kind::Method { receiver, .. } = function.kind else {
            return Err(Fallback::UnsupportedSource.into());
        };
        let Kind::Text(separator) = receiver.kind else {
            return Err(Fallback::UnsupportedSource.into());
        };
        // Zero/singleton joins must not discard a fallible separator recipe.
        if !matches!(&separator.recipe, TextRecipe::Literal { .. }) {
            return self.halt(node, PythonBarrierReason::UnsupportedOperation);
        }
        let mut arguments = parts(field(node, "arguments")?);
        let Some(argument) = arguments.next() else {
            return self.halt(node, PythonBarrierReason::UnsupportedOperation);
        };
        let sequence = unparenthesized(argument)?;
        if arguments.next().is_some() || !matches!(sequence.kind(), "list" | "tuple") {
            return self.halt(node, PythonBarrierReason::UnsupportedOperation);
        }
        let count = parts(sequence).count();
        if count > MAX_RECORDS {
            return Err(Fallback::SourceLimit.into());
        }
        // Literal evidence promises an original token. An empty join has no
        // such token for its derived empty string, so this subset refuses it.
        if count == 0 {
            return self.halt(node, PythonBarrierReason::UnsupportedOperation);
        }
        self.guard
            .charge(count.checked_mul(128).ok_or(Fallback::SourceLimit)?)?;
        let mut elements = Vec::with_capacity(count);
        let mut dependencies = function.dependencies;
        for element in parts(sequence) {
            let value = match pure::arm(
                element,
                self.source,
                self.source_id,
                &self.bindings,
                &mut self.guard,
            ) {
                Ok(value) => value,
                Err(PureError::Unsupported) => {
                    return self.halt(element, PythonBarrierReason::UnsupportedOperation);
                }
                Err(PureError::Unavailable(reason)) => return Err(reason.into()),
            };
            let Kind::Text(text) = value.kind else {
                return Err(Fallback::UnsupportedSource.into());
            };
            self.merge(&mut dependencies, value.dependencies)?;
            elements.push(text);
        }
        self.dependency(
            &mut dependencies,
            PythonDependency::Capability {
                assumption: PythonAssumption::StandardTextMethods,
            },
        )?;
        let length = count
            .checked_mul(2)
            .and_then(|n| n.checked_sub(1))
            .ok_or(Fallback::SourceLimit)?;
        self.guard
            .charge(length.checked_mul(128).ok_or(Fallback::SourceLimit)?)?;
        let mut leaves = Vec::with_capacity(length);
        for (index, element) in elements.into_iter().enumerate() {
            if index > 0 {
                // Keep the captured separator; every retained clone is charged.
                self.guard.charge(separator.weight)?;
                leaves.push(separator.clone());
            }
            leaves.push(element);
        }
        let text = self.join_tree(node, &mut leaves.into_iter(), length)?;
        // This call must complete even when its result is later discarded.
        self.condition(node)?;
        Ok(Value::new(Kind::Text(text), dependencies))
    }

    fn join_tree(
        &mut self,
        node: Node<'_>,
        leaves: &mut std::vec::IntoIter<TextValue>,
        length: usize,
    ) -> Result<TextValue> {
        self.guard.charge(0)?;
        if length == 1 {
            return leaves
                .next()
                .ok_or_else(|| Fallback::UnsupportedSource.into());
        }
        let left_length = length.div_ceil(2);
        let left = self.join_tree(node, leaves, left_length)?;
        let right = self.join_tree(node, leaves, length - left_length)?;
        let depth = left.depth.max(right.depth) + 1;
        if depth > MAX_RECIPE_DEPTH {
            return Err(Fallback::SourceLimit.into());
        }
        let weight = left
            .weight
            .checked_add(right.weight)
            .and_then(|value| value.checked_add(128))
            .ok_or(Fallback::SourceLimit)?;
        self.guard.charge(128)?;
        Ok(TextValue {
            recipe: TextRecipe::Concat {
                left: Box::new(left.recipe),
                right: Box::new(right.recipe),
                evidence: self.evidence(node),
            },
            depth,
            weight,
        })
    }
}
