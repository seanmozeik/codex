//! Closed branch recipes. This builder cannot access or mutate execution state.

mod index;
mod predicates;
mod replace;
mod slices;

use super::Binding;
use super::Guard;
use super::Kind;
use super::MAX_RECIPE_DEPTH;
use super::MAX_RECORDS;
use super::Method;
use super::TextValue;
use super::Value;
use super::dependency_weight;
use crate::SourceDescriptionFallback as Fallback;
use crate::SourceId;
use crate::selective_edits::python::PythonAssumption;
use crate::selective_edits::python::PythonDependency;
use crate::selective_edits::python::TextRecipe;
use crate::selective_edits::python::literal;
use crate::selective_edits::python::syntax::field;
use crate::selective_edits::python::syntax::unparenthesized;
use std::collections::BTreeMap;
use tree_sitter::Node;

pub(super) enum PureError {
    Unsupported,
    Unavailable(Fallback),
}

impl From<Fallback> for PureError {
    fn from(reason: Fallback) -> Self {
        Self::Unavailable(reason)
    }
}

type Result<T> = std::result::Result<T, PureError>;

pub(super) fn arm(
    node: Node<'_>,
    source: &str,
    source_id: SourceId,
    bindings: &BTreeMap<String, Binding>,
    guard: &mut Guard,
) -> Result<Value> {
    Builder {
        source,
        source_id,
        bindings,
        guard,
    }
    .text(node, 0)
}

pub(super) fn predicate(
    node: Node<'_>,
    source: &str,
    source_id: SourceId,
    bindings: &BTreeMap<String, Binding>,
    guard: &mut Guard,
) -> Result<Value> {
    Builder {
        source,
        source_id,
        bindings,
        guard,
    }
    .predicate(node, 0)
}

struct Builder<'s, 'g> {
    source: &'s str,
    source_id: SourceId,
    bindings: &'s BTreeMap<String, Binding>,
    guard: &'g mut Guard,
}

impl Builder<'_, '_> {
    fn text(&mut self, node: Node<'_>, depth: usize) -> Result<Value> {
        if depth >= MAX_RECIPE_DEPTH {
            return Err(Fallback::SourceLimit.into());
        }
        self.guard.charge(128)?;
        let node = unparenthesized(node)?;
        match node.kind() {
            "string" => {
                self.guard.charge(crate::text(node, self.source).len())?;
                let value = literal::decode(crate::text(node, self.source))?;
                let weight = value.len() + 128;
                Ok(Value::new(
                    Kind::Text(TextValue {
                        recipe: TextRecipe::Literal {
                            value,
                            evidence: crate::span(node, self.source_id),
                        },
                        depth: 1,
                        weight,
                    }),
                    Vec::new(),
                ))
            }
            "identifier" => {
                let value = self.lookup(node)?;
                if matches!(&value.kind, Kind::Text(_)) {
                    Ok(value)
                } else {
                    Err(PureError::Unsupported)
                }
            }
            "binary_operator" => {
                if crate::text(field(node, "operator")?, self.source) != "+" {
                    return Err(PureError::Unsupported);
                }
                let left = self.text(field(node, "left")?, depth + 1)?;
                let right = self.text(field(node, "right")?, depth + 1)?;
                let mut dependencies = left.dependencies;
                self.merge(&mut dependencies, right.dependencies)?;
                let (Kind::Text(left), Kind::Text(right)) = (left.kind, right.kind) else {
                    return Err(PureError::Unsupported);
                };
                let recipe = TextRecipe::Concat {
                    left: Box::new(left.recipe),
                    right: Box::new(right.recipe),
                    evidence: crate::span(node, self.source_id),
                };
                self.finish(
                    recipe,
                    left.depth.max(right.depth) + 1,
                    left.weight + right.weight + 128,
                    dependencies,
                )
            }
            "call" => self.replace(node, depth + 1),
            "subscript" => self.slice(node, depth + 1),
            // Fresh reads, nested conditionals and arbitrary calls
            // are outside this closed arm language, even if unselected.
            _ => Err(PureError::Unsupported),
        }
    }

    fn lookup(&mut self, node: Node<'_>) -> Result<Value> {
        let name = crate::text(node, self.source);
        let binding = self.bindings.get(name).ok_or(PureError::Unsupported)?;
        self.guard.charge(binding.value.weight + name.len() + 192)?;
        let mut value = binding.value.clone();
        let version = binding.version;
        let evidence = binding.evidence.clone();
        self.dependency(
            &mut value.dependencies,
            PythonDependency::Binding {
                name: name.to_owned(),
                version,
                evidence,
            },
        )?;
        Ok(Value::new(value.kind, value.dependencies))
    }

    fn receiver(
        &mut self,
        function: Node<'_>,
        expected: Method,
        depth: usize,
    ) -> Result<(TextValue, Vec<PythonDependency>)> {
        let function = unparenthesized(function)?;
        let (kind, dependencies) = match function.kind() {
            "attribute" => {
                let name = crate::text(field(function, "attribute")?, self.source);
                if !matches!(
                    (expected, name),
                    (Method::Replace, "replace")
                        | (Method::Index, "index")
                        | (Method::StartsWith, "startswith")
                        | (Method::EndsWith, "endswith")
                ) {
                    return Err(PureError::Unsupported);
                }
                let value = self.text(field(function, "object")?, depth)?;
                (value.kind, value.dependencies)
            }
            "identifier" => {
                let value = self.lookup(function)?;
                let Kind::Method { receiver, method } = value.kind else {
                    return Err(PureError::Unsupported);
                };
                if !matches!(
                    (expected, method),
                    (Method::Replace, Method::Replace)
                        | (Method::Index, Method::Index)
                        | (Method::StartsWith, Method::StartsWith)
                        | (Method::EndsWith, Method::EndsWith)
                ) {
                    return Err(PureError::Unsupported);
                }
                (receiver.kind, value.dependencies)
            }
            _ => return Err(PureError::Unsupported),
        };
        let Kind::Text(input) = kind else {
            return Err(PureError::Unsupported);
        };
        Ok((input, dependencies))
    }

    fn finish(
        &mut self,
        recipe: TextRecipe,
        depth: usize,
        weight: usize,
        mut dependencies: Vec<PythonDependency>,
    ) -> Result<Value> {
        if depth > MAX_RECIPE_DEPTH {
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
            Kind::Text(TextValue {
                recipe,
                depth,
                weight,
            }),
            dependencies,
        ))
    }

    fn dependency(
        &mut self,
        into: &mut Vec<PythonDependency>,
        value: PythonDependency,
    ) -> Result<()> {
        if !into.contains(&value) {
            if into.len() >= MAX_RECORDS {
                return Err(Fallback::SourceLimit.into());
            }
            self.guard.charge(dependency_weight(&value))?;
            into.push(value);
        }
        Ok(())
    }

    fn merge(
        &mut self,
        into: &mut Vec<PythonDependency>,
        values: Vec<PythonDependency>,
    ) -> Result<()> {
        for value in values {
            self.dependency(into, value)?;
        }
        Ok(())
    }
}
