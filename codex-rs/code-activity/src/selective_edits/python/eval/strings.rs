//! Bounded immutable string recipes. No source program or file contents execute.

mod subscript;

use super::Kind;
use super::MAX_RECIPE_DEPTH;
use super::Method;
use super::Result;
use super::State;
use super::TextValue;
use super::Value;
use super::calls::Argument;
use crate::SourceDescriptionFallback as Fallback;
use crate::Span;
use crate::selective_edits::python::IndexRecipe;
use crate::selective_edits::python::PythonAssumption;
use crate::selective_edits::python::PythonBarrierReason;
use crate::selective_edits::python::PythonDependency;
use crate::selective_edits::python::TextRecipe;
use tree_sitter::Node;

#[derive(Clone)]
pub(super) struct IndexValue {
    pub(super) recipe: IndexRecipe,
    pub(super) depth: usize,
    pub(super) weight: usize,
}

#[derive(Clone)]
pub(super) struct SplitValue {
    pub(super) input: TextValue,
    pub(super) separator: String,
    pub(super) maxsplit: Option<i64>,
    pub(super) evidence: Span,
    pub(super) weight: usize,
}

impl State<'_> {
    pub(super) fn string_method(
        &mut self,
        node: Node<'_>,
        receiver: Value,
        method: Method,
        arguments: Vec<Argument>,
        mut dependencies: Vec<PythonDependency>,
    ) -> Result<Value> {
        if arguments.iter().any(|argument| argument.name.is_some()) {
            return self.halt(node, PythonBarrierReason::UnsupportedOperation);
        }
        let Kind::Text(input) = receiver.kind else {
            return self.halt(node, PythonBarrierReason::UnsupportedOperation);
        };
        self.dependency(
            &mut dependencies,
            PythonDependency::Capability {
                assumption: PythonAssumption::StandardTextMethods,
            },
        )?;
        match method {
            Method::Index if (1..=2).contains(&arguments.len()) => {
                let Some(needle) = arguments[0].value.literal() else {
                    return self.halt(node, PythonBarrierReason::UnsupportedOperation);
                };
                if let TextRecipe::Literal { value, .. } = &input.recipe
                    && !value.contains(needle)
                {
                    return self.halt(node, PythonBarrierReason::UnsupportedOperation);
                }
                let start = if arguments.len() == 2 {
                    Some(self.index_value(
                        node,
                        &arguments[1].value,
                        arguments[1].evidence.clone(),
                    )?)
                } else {
                    None
                };
                if let TextRecipe::Literal { value, .. } = &input.recipe {
                    let constant_start = match start.as_ref().map(|v| &v.recipe) {
                        None => Some(0),
                        Some(IndexRecipe::Literal { value, .. }) => Some(*value),
                        _ => None,
                    };
                    if let Some(offset) = constant_start {
                        // Python indexes Unicode code points. A positive start
                        // beyond len still fails for an empty search string.
                        let length = value.chars().count() as i128;
                        let offset = i128::from(offset);
                        let normalized = if offset < 0 {
                            (length + offset).max(0)
                        } else {
                            offset
                        };
                        if normalized > length {
                            return self.halt(node, PythonBarrierReason::UnsupportedOperation);
                        }
                        let byte = value
                            .char_indices()
                            .nth(normalized as usize)
                            .map_or(value.len(), |(byte, _)| byte);
                        if !value[byte..].contains(needle) {
                            return self.halt(node, PythonBarrierReason::UnsupportedOperation);
                        }
                    }
                }
                let depth = input
                    .depth
                    .max(start.as_ref().map_or(0, |value| value.depth))
                    + 1;
                self.string_depth(depth)?;
                let weight = input.weight
                    + needle.len()
                    + start.as_ref().map_or(0, |value| value.weight)
                    + 128;
                self.guard.charge(needle.len() + 128)?;
                // An absent needle raises ValueError. This successful-completion
                // guard is required even if the integer result is later discarded.
                self.condition(node)?;
                Ok(Value::new(
                    Kind::Index(IndexValue {
                        recipe: IndexRecipe::SubstringIndex {
                            input: Box::new(input.recipe),
                            needle: needle.to_owned(),
                            start: start.map(|value| Box::new(value.recipe)),
                            evidence: self.evidence(node),
                        },
                        depth,
                        weight,
                    }),
                    dependencies,
                ))
            }
            Method::Split if (1..=2).contains(&arguments.len()) => {
                let Some(separator) = arguments[0].value.literal() else {
                    return self.halt(node, PythonBarrierReason::UnsupportedOperation);
                };
                // Python's empty separator always raises; no later write is
                // forecast. Negative maxsplit retains Python's unlimited meaning.
                if separator.is_empty() {
                    return self.halt(node, PythonBarrierReason::UnsupportedOperation);
                }
                let maxsplit = if arguments.len() == 2 {
                    let Kind::Integer(Some(value)) = &arguments[1].value.kind else {
                        return self.halt(node, PythonBarrierReason::UnsupportedOperation);
                    };
                    Some(*value)
                } else {
                    None
                };
                self.string_depth(input.depth + 1)?;
                let weight = input.weight + separator.len() + 128;
                self.guard.charge(separator.len() + 128)?;
                Ok(Value::new(
                    Kind::ListText(SplitValue {
                        input,
                        separator: separator.to_owned(),
                        maxsplit,
                        evidence: self.evidence(node),
                        weight,
                    }),
                    dependencies,
                ))
            }
            Method::Rstrip if arguments.len() == 1 => {
                let Some(characters) = arguments[0].value.literal() else {
                    return self.halt(node, PythonBarrierReason::UnsupportedOperation);
                };
                let depth = input.depth + 1;
                let weight = input.weight + characters.len() + 128;
                self.string_depth(depth)?;
                self.guard.charge(characters.len() + 128)?;
                Ok(Value::new(
                    Kind::Text(TextValue {
                        recipe: TextRecipe::Rstrip {
                            input: Box::new(input.recipe),
                            characters: characters.to_owned(),
                            evidence: self.evidence(node),
                        },
                        depth,
                        weight,
                    }),
                    dependencies,
                ))
            }
            _ => self.halt(node, PythonBarrierReason::UnsupportedOperation),
        }
    }

    fn string_depth(&self, depth: usize) -> Result<()> {
        if depth > MAX_RECIPE_DEPTH {
            Err(Fallback::SourceLimit.into())
        } else {
            Ok(())
        }
    }

    fn index_value(&mut self, node: Node<'_>, value: &Value, evidence: Span) -> Result<IndexValue> {
        match &value.kind {
            Kind::Integer(Some(value)) => Ok(IndexValue {
                recipe: IndexRecipe::Literal {
                    value: *value,
                    evidence,
                },
                depth: 1,
                weight: 128,
            }),
            Kind::Index(value) => {
                self.guard.charge(value.weight)?;
                Ok(value.clone())
            }
            _ => self.halt(node, PythonBarrierReason::UnsupportedOperation),
        }
    }

    pub(super) fn replace_text(
        &mut self,
        node: Node<'_>,
        input: TextValue,
        old: &Value,
        new: &Value,
        count: Option<usize>,
        mut dependencies: Vec<PythonDependency>,
    ) -> Result<Value> {
        let (Kind::Text(old), Kind::Text(new)) = (&old.kind, &new.kind) else {
            return self.halt(node, PythonBarrierReason::UnsupportedOperation);
        };
        let depth = input.depth.max(old.depth).max(new.depth) + 1;
        self.string_depth(depth)?;
        self.guard.charge(old.weight + new.weight + 128)?;
        self.dependency(
            &mut dependencies,
            PythonDependency::Capability {
                assumption: PythonAssumption::StandardTextMethods,
            },
        )?;
        let weight = input.weight + old.weight + new.weight + 128;
        Ok(Value::new(
            Kind::Text(TextValue {
                recipe: TextRecipe::ReplaceText {
                    input: Box::new(input.recipe),
                    old: Box::new(old.recipe.clone()),
                    new: Box::new(new.recipe.clone()),
                    count,
                    evidence: self.evidence(node),
                },
                depth,
                weight,
            }),
            dependencies,
        ))
    }
}
