//! Frozen primitive values, lexical dependencies and allocation accounting.

use super::Dependency;
use super::Fallback;
use super::FilesystemMethod;
use super::Kind;
use super::MAX_RECORDS;
use super::Reason;
use super::Recipe;
use super::Result;
use super::State;
use super::Use;
use super::Value;
use super::syntax;
use tree_sitter::Node;

impl State<'_> {
    pub(super) fn value(
        &mut self,
        kind: Kind,
        dependencies: Vec<Dependency>,
        weight: usize,
        depth: usize,
    ) -> Result<Value> {
        if depth > 64 || dependencies.len() > MAX_RECORDS {
            return Err(Fallback::SourceLimit.into());
        }
        self.charge(weight)?;
        Ok(Value {
            kind,
            dependencies,
            weight,
            depth,
        })
    }

    pub(super) fn lookup(&mut self, node: Node<'_>) -> Result<Value> {
        let name = crate::text(node, self.source);
        let Some(weight) = self.bindings.get(name).map(|binding| binding.value.weight) else {
            return self.halt(node, Reason::UnknownName);
        };
        self.charge(weight + name.len() + 128)?;
        let binding = self
            .bindings
            .get(name)
            .cloned()
            .ok_or(Fallback::UnsupportedSource)?;
        let mut value = binding.value;
        self.dependency(
            &mut value.dependencies,
            Dependency::Binding {
                name: name.to_owned(),
                version: binding.version,
                evidence: binding.evidence,
            },
        )?;
        value.weight += name.len() + 128;
        Ok(value)
    }

    pub(super) fn expression(&mut self, node: Node<'_>, usage: Use) -> Result<Value> {
        self.charge(0)?;
        match node.kind() {
            "identifier" => {
                let value = self.lookup(node)?;
                if let Kind::Descriptor(id) = &value.kind
                    && (usage != Use::InitializeConst || !self.descriptors.contains_key(id))
                {
                    return self.halt(node, Reason::UnsupportedOperation);
                }
                Ok(value)
            }
            "number" => self.value(
                Kind::Integer(syntax::integer(node, self.source)?),
                Vec::new(),
                64,
                0,
            ),
            "null" => self.value(Kind::Null, Vec::new(), 64, 0),
            "true" | "false" => self.value(Kind::Boolean(node.kind() == "true"), Vec::new(), 64, 0),
            "string" => {
                let value = syntax::literal(node, self.source)?;
                let weight = value.len() + 128;
                self.value(
                    Kind::Text(Recipe::Literal {
                        value,
                        evidence: self.evidence(node),
                    }),
                    Vec::new(),
                    weight,
                    1,
                )
            }
            "parenthesized_expression" => self.expression(syntax::only(node)?, usage),
            "member_expression" => {
                let (object, name) = syntax::member(node, self.source)?;
                let receiver = self.expression(object, Use::Value)?;
                let Some(method) = FilesystemMethod::from_name(name) else {
                    return self.halt(node, Reason::UnsupportedOperation);
                };
                if !matches!(receiver.kind, Kind::Filesystem) {
                    return self.halt(node, Reason::UnsupportedOperation);
                }
                self.value(
                    Kind::FilesystemFunction(method),
                    receiver.dependencies,
                    receiver.weight + 128,
                    receiver.depth + 1,
                )
            }
            "call_expression" => self.call(node, usage),
            "binary_expression" => {
                let operator = syntax::binary_operator(node, self.source)?;
                if operator != syntax::BinaryOperator::Add {
                    return self.lazy(node, operator, usage);
                }
                let left = self.expression(syntax::field(node, "left")?, Use::Value)?;
                let right = self.expression(syntax::field(node, "right")?, Use::Value)?;
                let (Kind::Text(left_text), Kind::Text(right_text)) = (left.kind, right.kind)
                else {
                    return self.halt(node, Reason::UnsupportedOperation);
                };
                let mut dependencies = left.dependencies;
                self.merge(&mut dependencies, right.dependencies)?;
                self.value(
                    Kind::Text(Recipe::Concat {
                        left: Box::new(left_text),
                        right: Box::new(right_text),
                        evidence: self.evidence(node),
                    }),
                    dependencies,
                    left.weight + right.weight + 128,
                    left.depth.max(right.depth) + 1,
                )
            }
            _ => self.halt(node, Reason::UnsupportedOperation),
        }
    }

    pub(super) fn constant(&mut self, value: &Value) -> Result<Option<String>> {
        fn append(recipe: &Recipe, output: &mut String) -> Option<()> {
            match recipe {
                Recipe::Literal { value, .. } => {
                    if output.len().checked_add(value.len())? > 16_384 {
                        return None;
                    }
                    output.push_str(value);
                }
                Recipe::Concat { left, right, .. } => {
                    append(left, output)?;
                    append(right, output)?;
                }
                _ => return None,
            }
            Some(())
        }
        self.charge(value.weight)?;
        let Kind::Text(recipe) = &value.kind else {
            return Ok(None);
        };
        let mut output = String::new();
        Ok(append(recipe, &mut output).map(|()| output))
    }

    pub(super) fn dependency(
        &mut self,
        dependencies: &mut Vec<Dependency>,
        value: Dependency,
    ) -> Result<()> {
        if !dependencies.contains(&value) {
            if dependencies.len() >= MAX_RECORDS {
                return Err(Fallback::SourceLimit.into());
            }
            self.charge(128)?;
            dependencies.push(value);
        }
        Ok(())
    }

    pub(super) fn merge(
        &mut self,
        dependencies: &mut Vec<Dependency>,
        additions: Vec<Dependency>,
    ) -> Result<()> {
        for dependency in additions {
            self.dependency(dependencies, dependency)?;
        }
        Ok(())
    }
}
