//! Predicate-first evaluation with separately bounded, effect-free arms.

use super::Kind;
use super::MAX_RECIPE_DEPTH;
use super::Result;
use super::State;
use super::TextValue;
use super::Value;
use super::pure;
use super::pure::PureError;
use crate::SourceDescriptionFallback as Fallback;
use crate::selective_edits::python::PythonBarrierReason;
use crate::selective_edits::python::TextRecipe;
use crate::selective_edits::python::syntax::conditional_parts;
use tree_sitter::Node;

impl State<'_> {
    pub(super) fn literal_lazy_or_predicate(&mut self, node: Node<'_>) -> Result<Value> {
        let left =
            super::super::syntax::unparenthesized(super::super::syntax::field(node, "left")?)?;
        let operator = crate::text(super::super::syntax::field(node, "operator")?, self.source);
        if matches!((left.kind(), operator), ("false", "and") | ("true", "or")) {
            self.condition(node)?;
            return Ok(Value::new(Kind::Primitive, Vec::new()));
        }
        self.closed_predicate(node)
    }

    pub(super) fn conditional(&mut self, node: Node<'_>) -> Result<Value> {
        let (consequence, predicate, alternative) = conditional_parts(node)?;
        // Only the predicate executes through State. Branch construction has
        // immutable binding access plus a resource guard, never State access.
        let predicate = self.expression(predicate)?;
        let Kind::Predicate(predicate_value) = predicate.kind else {
            return self.halt(node, PythonBarrierReason::UnsupportedOperation);
        };
        let consequence = self.closed_arm(consequence)?;
        let alternative = self.closed_arm(alternative)?;
        let mut dependencies = predicate.dependencies;
        self.merge(&mut dependencies, consequence.dependencies)?;
        self.merge(&mut dependencies, alternative.dependencies)?;
        let (Kind::Text(consequence), Kind::Text(alternative)) =
            (consequence.kind, alternative.kind)
        else {
            return Err(Fallback::UnsupportedSource.into());
        };
        let depth = predicate_value
            .depth
            .max(consequence.depth)
            .max(alternative.depth)
            + 1;
        if depth > MAX_RECIPE_DEPTH {
            return Err(Fallback::SourceLimit.into());
        }
        self.guard.charge(128)?;
        // A selected arm may raise from an index recipe. Later effects depend
        // on this whole expression returning, even if its value is discarded.
        // Arm-local index spans are not unconditional execution prerequisites.
        self.condition(node)?;
        let weight = predicate_value.weight + consequence.weight + alternative.weight + 128;
        Ok(Value::new(
            Kind::Text(TextValue {
                recipe: TextRecipe::Conditional {
                    predicate: Box::new(predicate_value.recipe),
                    consequence: Box::new(consequence.recipe),
                    alternative: Box::new(alternative.recipe),
                    evidence: self.evidence(node),
                },
                depth,
                weight,
            }),
            dependencies,
        ))
    }

    pub(super) fn closed_predicate(&mut self, node: Node<'_>) -> Result<Value> {
        let value = match pure::predicate(
            node,
            self.source,
            self.source_id,
            &self.bindings,
            &mut self.guard,
        ) {
            Ok(value) => value,
            Err(PureError::Unsupported) => {
                return self.halt(node, PythonBarrierReason::UnsupportedOperation);
            }
            Err(PureError::Unavailable(reason)) => return Err(reason.into()),
        };
        // The whole predicate must return before later effects. A lazy RHS
        // index/call is not an unconditional execution prerequisite.
        self.condition(node)?;
        Ok(value)
    }

    fn closed_arm(&mut self, node: Node<'_>) -> Result<Value> {
        match pure::arm(
            node,
            self.source,
            self.source_id,
            &self.bindings,
            &mut self.guard,
        ) {
            Ok(value) => Ok(value),
            Err(PureError::Unsupported) => {
                self.halt(node, PythonBarrierReason::UnsupportedOperation)
            }
            Err(PureError::Unavailable(reason)) => Err(reason.into()),
        }
    }
}
