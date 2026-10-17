//! Finite standard text calls whose scalar output need not be computed.

use super::Kind;
use super::MAX_RECIPE_DEPTH;
use super::Method;
use super::Result;
use super::State;
use super::TextValue;
use super::Value;
use super::calls::Argument;
use crate::SourceDescriptionFallback as Fallback;
use crate::selective_edits::python::PredicateRecipe;
use crate::selective_edits::python::PythonAssumption;
use crate::selective_edits::python::PythonBarrierReason;
use crate::selective_edits::python::PythonDependency;
use crate::selective_edits::python::TextRecipe;
use tree_sitter::Node;

#[derive(Clone)]
pub(super) struct PredicateValue {
    pub(super) recipe: PredicateRecipe,
    pub(super) depth: usize,
    pub(super) weight: usize,
}

impl State<'_> {
    pub(super) fn summarized_method(
        &mut self,
        node: Node<'_>,
        receiver: Value,
        method: Method,
        arguments: Vec<Argument>,
        mut dependencies: Vec<PythonDependency>,
    ) -> Result<Value> {
        // Arguments have already run in Python order. Only this closed, total
        // signature establishes local effects under StandardTextMethods.
        if arguments.len() != 1 || arguments[0].name.is_some() {
            return self.halt(node, PythonBarrierReason::UnsupportedOperation);
        }
        let argument = arguments
            .into_iter()
            .next()
            .ok_or(Fallback::UnsupportedSource)?;
        let Kind::Text(TextValue {
            recipe: TextRecipe::Literal { value, .. },
            ..
        }) = argument.value.kind
        else {
            return self.halt(node, PythonBarrierReason::UnsupportedOperation);
        };
        let Kind::Text(input) = receiver.kind else {
            return Err(Fallback::UnsupportedSource.into());
        };
        self.dependency(
            &mut dependencies,
            PythonDependency::Capability {
                assumption: PythonAssumption::StandardTextMethods,
            },
        )?;
        self.condition(node)?;
        if matches!(method, Method::Count | Method::Find) {
            // Unknown integers are real bindings. Rebinding `open` must never
            // remove the name and thereby reveal the initial builtin again.
            return Ok(Value::new(Kind::Integer(None), dependencies));
        }
        if input.depth >= MAX_RECIPE_DEPTH {
            return Err(Fallback::SourceLimit.into());
        }
        let weight = input.weight + value.len() + 128;
        self.guard.charge(128)?;
        let recipe = match method {
            Method::StartsWith => PredicateRecipe::StartsWith {
                input: Box::new(input.recipe),
                prefix: value,
                prefix_evidence: argument.evidence,
                evidence: self.evidence(node),
            },
            Method::EndsWith => PredicateRecipe::EndsWith {
                input: Box::new(input.recipe),
                suffix: value,
                suffix_evidence: argument.evidence,
                evidence: self.evidence(node),
            },
            _ => return Err(Fallback::UnsupportedSource.into()),
        };
        Ok(Value::new(
            Kind::Predicate(PredicateValue {
                recipe,
                depth: input.depth + 1,
                weight,
            }),
            dependencies,
        ))
    }
}
