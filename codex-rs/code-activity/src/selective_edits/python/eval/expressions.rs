//! Ordered expression evaluation and immutable text composition.

use super::Kind;
use super::MAX_RECIPE_DEPTH;
use super::Method;
use super::Mode;
use super::Result;
use super::State;
use super::TextValue;
use super::Value;
use crate::SourceDescriptionFallback as Fallback;
use crate::selective_edits::python::PythonAssumption;
use crate::selective_edits::python::PythonBarrierReason;
use crate::selective_edits::python::PythonDependency;
use crate::selective_edits::python::TextRecipe;
use crate::selective_edits::python::literal;
use crate::selective_edits::python::syntax::field;
use crate::selective_edits::python::syntax::parts;
use crate::selective_edits::python::syntax::unparenthesized;
use tree_sitter::Node;

impl State<'_> {
    pub(super) fn lookup(&mut self, node: Node<'_>) -> Result<Value> {
        let name = crate::text(node, self.source);
        if let Some(binding) = self.bindings.get(name) {
            self.guard.charge(binding.value.weight + name.len() + 192)?;
            let binding = binding.clone();
            let mut dependencies = binding.value.dependencies;
            self.dependency(
                &mut dependencies,
                PythonDependency::Binding {
                    name: name.to_owned(),
                    version: binding.version,
                    evidence: binding.evidence,
                },
            )?;
            return Ok(Value::new(binding.value.kind, dependencies));
        }
        if name == "open" {
            self.guard.charge(256)?;
            return Ok(Value::new(
                Kind::OpenFunction,
                vec![PythonDependency::Capability {
                    assumption: PythonAssumption::StandardBuiltinOpen,
                }],
            ));
        }
        self.halt(node, PythonBarrierReason::UnknownName)
    }

    pub(super) fn expression(&mut self, node: Node<'_>) -> Result<Value> {
        self.guard.charge(128)?;
        let node = unparenthesized(node)?;
        match node.kind() {
            "identifier" => self.lookup(node),
            "string" => {
                self.guard.charge(crate::text(node, self.source).len())?;
                let value = literal::decode(crate::text(node, self.source))?;
                let weight = value.len() + 128;
                Ok(Value::new(
                    Kind::Text(TextValue {
                        recipe: TextRecipe::Literal {
                            value,
                            evidence: self.evidence(node),
                        },
                        depth: 1,
                        weight,
                    }),
                    Vec::new(),
                ))
            }
            "integer" => Ok(Value::new(
                Kind::Integer(Some(literal::integer(crate::text(node, self.source))?)),
                Vec::new(),
            )),
            "true" | "false" | "none" => Ok(Value::new(Kind::Primitive, Vec::new())),
            "unary_operator" => {
                let value = literal::integer(crate::text(
                    unparenthesized(field(node, "argument")?)?,
                    self.source,
                ))?;
                let signed = if crate::text(field(node, "operator")?, self.source) == "-" {
                    value.checked_neg().ok_or(Fallback::UnsupportedSource)?
                } else {
                    value
                };
                Ok(Value::new(Kind::Integer(Some(signed)), Vec::new()))
            }
            "list" | "tuple" | "dictionary" => self.container(node),
            "attribute" => self.attribute(node),
            "call" => self.call(node),
            "binary_operator" => self.concat(node),
            "subscript" => self.string_subscript(node),
            "conditional_expression" => self.conditional(node),
            "comparison_operator" => self.closed_predicate(node),
            "boolean_operator" => self.literal_lazy_or_predicate(node),
            _ => Err(Fallback::UnsupportedSource.into()),
        }
    }

    fn container(&mut self, node: Node<'_>) -> Result<Value> {
        let mut dependencies = Vec::new();
        let mut hashable = node.kind() == "tuple";
        for item in parts(node) {
            if node.kind() == "dictionary" {
                let key = self.expression(field(item, "key")?)?;
                if key.has_handle() {
                    return self.halt(node, PythonBarrierReason::EscapingHandle);
                }
                let value = self.expression(field(item, "value")?)?;
                if value.has_handle() {
                    return self.halt(node, PythonBarrierReason::EscapingHandle);
                }
                // Python evaluates the value before hashing/inserting this key.
                if !key.hashable() {
                    return self.halt(node, PythonBarrierReason::UnsupportedOperation);
                }
                self.merge(&mut dependencies, key.dependencies)?;
                self.merge(&mut dependencies, value.dependencies)?;
            } else {
                let value = self.expression(item)?;
                if value.has_handle() {
                    return self.halt(node, PythonBarrierReason::EscapingHandle);
                }
                hashable &= value.hashable();
                self.merge(&mut dependencies, value.dependencies)?;
            }
        }
        Ok(Value::new(Kind::Container { hashable }, dependencies))
    }

    fn attribute(&mut self, node: Node<'_>) -> Result<Value> {
        let receiver = self.expression(field(node, "object")?)?;
        let name = crate::text(field(node, "attribute")?, self.source);
        let method = match (&receiver.kind, name) {
            (Kind::PathlibModule, "Path") => {
                return Ok(Value::new(Kind::PathFunction, receiver.dependencies));
            }
            (Kind::Path(_), "read_text") => Method::PathRead,
            (Kind::Path(_), "write_text") => Method::PathWrite,
            (Kind::ContextHandle(_), "read") => Method::ContextRead,
            (Kind::ContextHandle(_), "write") => Method::ContextWrite,
            (Kind::ContextHandle(_), "close") => Method::ContextClose,
            (Kind::ContextHandle(_), "seek") => Method::ContextSeek,
            (Kind::Text(_), "replace") => Method::Replace,
            (Kind::Text(_), "join") => Method::Join,
            (Kind::Text(_), "index") => Method::Index,
            (Kind::Text(_), "split") => Method::Split,
            (Kind::Text(_), "rstrip") => Method::Rstrip,
            (Kind::Text(_), "count") => Method::Count,
            (Kind::Text(_), "find") => Method::Find,
            (Kind::Text(_), "startswith") => Method::StartsWith,
            (Kind::Text(_), "endswith") => Method::EndsWith,
            (
                Kind::Handle {
                    mode: Mode::Read, ..
                },
                "read",
            ) => Method::OpenRead,
            (
                Kind::Handle {
                    mode: Mode::Write, ..
                },
                "write",
            ) => Method::OpenWrite,
            _ => return self.halt(node, PythonBarrierReason::UnsupportedOperation),
        };
        // Method bindings freeze the receiver value rather than re-reading its
        // source variable after an unrelated local rebind.
        let dependencies = self.copy_dependencies(&receiver.dependencies)?;
        Ok(Value::new(
            Kind::Method {
                receiver: Box::new(receiver),
                method,
            },
            dependencies,
        ))
    }

    fn concat(&mut self, node: Node<'_>) -> Result<Value> {
        let left = self.expression(field(node, "left")?)?;
        let right = self.expression(field(node, "right")?)?;
        self.concat_values(node, left, right)
    }

    pub(super) fn concat_values(
        &mut self,
        node: Node<'_>,
        left: Value,
        right: Value,
    ) -> Result<Value> {
        let mut dependencies = left.dependencies;
        self.merge(&mut dependencies, right.dependencies)?;
        let (Kind::Text(left), Kind::Text(right)) = (left.kind, right.kind) else {
            return self.halt(node, PythonBarrierReason::UnsupportedOperation);
        };
        self.dependency(
            &mut dependencies,
            PythonDependency::Capability {
                assumption: PythonAssumption::StandardTextMethods,
            },
        )?;
        let depth = left.depth.max(right.depth) + 1;
        if depth > MAX_RECIPE_DEPTH {
            return Err(Fallback::SourceLimit.into());
        }
        let weight = left.weight + right.weight + 128;
        self.guard.charge(128)?;
        Ok(Value::new(
            Kind::Text(TextValue {
                recipe: TextRecipe::Concat {
                    left: Box::new(left.recipe),
                    right: Box::new(right.recipe),
                    evidence: self.evidence(node),
                },
                depth,
                weight,
            }),
            dependencies,
        ))
    }
}
