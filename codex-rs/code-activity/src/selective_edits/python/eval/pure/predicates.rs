//! Build boolean-only predicates without execution-state or filesystem access.

use super::super::MAX_RETAINED_BYTES;
use super::super::summaries::PredicateValue;
use super::Builder;
use super::Kind;
use super::MAX_RECIPE_DEPTH;
use super::Method;
use super::PureError;
use super::Result;
use super::Value;
use crate::SourceDescriptionFallback as Fallback;
use crate::selective_edits::python::PredicateRecipe;
use crate::selective_edits::python::PythonAssumption;
use crate::selective_edits::python::PythonDependency;
use crate::selective_edits::python::TextRecipe;
use crate::selective_edits::python::syntax::field;
use crate::selective_edits::python::syntax::only;
use crate::selective_edits::python::syntax::predicates::TextComparison;
use crate::selective_edits::python::syntax::predicates::comparison_parts;
use crate::selective_edits::python::syntax::predicates::conjunction_parts;
use crate::selective_edits::python::syntax::unparenthesized;
use tree_sitter::Node;

impl Builder<'_, '_> {
    pub(super) fn predicate(&mut self, node: Node<'_>, depth: usize) -> Result<Value> {
        if depth >= MAX_RECIPE_DEPTH {
            return Err(Fallback::SourceLimit.into());
        }
        self.guard.charge(128)?;
        let node = unparenthesized(node)?;
        match node.kind() {
            "identifier" => {
                let value = self.lookup(node)?;
                if matches!(&value.kind, Kind::Predicate(_)) {
                    Ok(value)
                } else {
                    Err(PureError::Unsupported)
                }
            }
            "comparison_operator" => self.comparison(node, depth + 1),
            "boolean_operator" => self.conjunction(node, depth + 1),
            "call" => self.predicate_call(node, depth + 1),
            // No arbitrary truth coercion, fresh read, or unknown call, even
            // when a concrete left operand would skip this expression.
            _ => Err(PureError::Unsupported),
        }
    }

    fn comparison(&mut self, node: Node<'_>, depth: usize) -> Result<Value> {
        let (operator, left, right) = comparison_parts(node, self.source)?;
        let left = self.text(left, depth)?;
        let right = self.text(right, depth)?;
        let mut dependencies = left.dependencies;
        self.merge(&mut dependencies, right.dependencies)?;
        let (Kind::Text(left), Kind::Text(right)) = (left.kind, right.kind) else {
            return Err(PureError::Unsupported);
        };
        let recipe_depth = left.depth.max(right.depth) + 1;
        let weight = combined_weight(left.weight, right.weight)?;
        let evidence = crate::span(node, self.source_id);
        let recipe = match operator {
            TextComparison::Contains => PredicateRecipe::TextContains {
                needle: Box::new(left.recipe),
                haystack: Box::new(right.recipe),
                evidence,
            },
            TextComparison::Equals => PredicateRecipe::TextEquals {
                left: Box::new(left.recipe),
                right: Box::new(right.recipe),
                evidence,
            },
        };
        self.finish_predicate(recipe, recipe_depth, weight, dependencies)
    }

    fn conjunction(&mut self, node: Node<'_>, depth: usize) -> Result<Value> {
        let (left, right) = conjunction_parts(node, self.source)?;
        let left = self.predicate(left, depth)?;
        let right = self.predicate(right, depth)?;
        let mut dependencies = left.dependencies;
        self.merge(&mut dependencies, right.dependencies)?;
        let (Kind::Predicate(left), Kind::Predicate(right)) = (left.kind, right.kind) else {
            return Err(PureError::Unsupported);
        };
        let recipe_depth = left.depth.max(right.depth) + 1;
        let weight = combined_weight(left.weight, right.weight)?;
        let recipe = PredicateRecipe::And {
            left: Box::new(left.recipe),
            right: Box::new(right.recipe),
            evidence: crate::span(node, self.source_id),
        };
        self.finish_predicate(recipe, recipe_depth, weight, dependencies)
    }

    fn predicate_call(&mut self, node: Node<'_>, depth: usize) -> Result<Value> {
        let function = unparenthesized(field(node, "function")?)?;
        let method = match function.kind() {
            "attribute" => match crate::text(field(function, "attribute")?, self.source) {
                "startswith" => Method::StartsWith,
                "endswith" => Method::EndsWith,
                _ => return Err(PureError::Unsupported),
            },
            "identifier" => {
                let name = crate::text(function, self.source);
                match self.bindings.get(name).map(|binding| &binding.value.kind) {
                    Some(Kind::Method {
                        method: Method::StartsWith,
                        ..
                    }) => Method::StartsWith,
                    Some(Kind::Method {
                        method: Method::EndsWith,
                        ..
                    }) => Method::EndsWith,
                    _ => return Err(PureError::Unsupported),
                }
            }
            _ => return Err(PureError::Unsupported),
        };
        // This check captures the actual receiver and callee binding version.
        // A method spelling alone never supplies authority.
        let (input, mut dependencies) = self.receiver(function, method, depth)?;
        let arguments = field(node, "arguments")?;
        if arguments.kind() != "argument_list" {
            return Err(PureError::Unsupported);
        }
        let argument = only(arguments)?;
        let value = self.text(argument, depth)?;
        self.merge(&mut dependencies, value.dependencies)?;
        let Kind::Text(value) = value.kind else {
            return Err(PureError::Unsupported);
        };
        let TextRecipe::Literal { value, .. } = value.recipe else {
            return Err(PureError::Unsupported);
        };
        let recipe_depth = input.depth + 1;
        let weight = combined_weight(input.weight, value.len())?;
        let evidence = crate::span(node, self.source_id);
        let value_evidence = crate::span(argument, self.source_id);
        let recipe = match method {
            Method::StartsWith => PredicateRecipe::StartsWith {
                input: Box::new(input.recipe),
                prefix: value,
                prefix_evidence: value_evidence,
                evidence,
            },
            Method::EndsWith => PredicateRecipe::EndsWith {
                input: Box::new(input.recipe),
                suffix: value,
                suffix_evidence: value_evidence,
                evidence,
            },
            _ => return Err(PureError::Unsupported),
        };
        self.finish_predicate(recipe, recipe_depth, weight, dependencies)
    }

    fn finish_predicate(
        &mut self,
        recipe: PredicateRecipe,
        depth: usize,
        weight: usize,
        mut dependencies: Vec<PythonDependency>,
    ) -> Result<Value> {
        if depth > MAX_RECIPE_DEPTH || weight > MAX_RETAINED_BYTES {
            return Err(Fallback::SourceLimit.into());
        }
        self.guard.charge(128)?;
        self.dependency(
            &mut dependencies,
            PythonDependency::Capability {
                assumption: PythonAssumption::StandardTextMethods,
            },
        )?;
        Ok(Value::new(
            Kind::Predicate(PredicateValue {
                recipe,
                depth,
                weight,
            }),
            dependencies,
        ))
    }
}

fn combined_weight(left: usize, right: usize) -> Result<usize> {
    left.checked_add(right)
        .and_then(|weight| weight.checked_add(128))
        .ok_or_else(|| Fallback::SourceLimit.into())
}
