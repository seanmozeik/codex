//! Complete finite CST recognizer, not a search for interesting callsites.

use super::Deadline;
use super::OrchestrationChildId;
use super::OrchestrationFallback;
use super::OrchestrationJoin;
use super::input::MAX_VALUES;
use super::names::reserved_binding;
use super::strings;
use crate::SourceId;
use crate::Span;
use crate::text;
use std::collections::BTreeMap;
use tree_sitter::Node;

const UNSUPPORTED: OrchestrationFallback = OrchestrationFallback::UnsupportedSource;
type Result<T> = std::result::Result<T, OrchestrationFallback>;

pub(super) struct Child {
    pub id: OrchestrationChildId,
    pub call_span: Span,
    pub command_span: Span,
    pub command: String,
}

pub(super) struct Group {
    pub join: OrchestrationJoin,
    pub span: Span,
    pub children: Vec<Child>,
}

enum Binding {
    Text(String),
    OpaqueResult,
}

struct Reader<'a> {
    source: &'a str,
    deadline: Deadline,
    bindings: BTreeMap<String, Binding>,
    children: usize,
    groups: Vec<Group>,
}

pub(super) fn extract(root: Node<'_>, source: &str, deadline: Deadline) -> Result<Vec<Group>> {
    let mut reader = Reader {
        source,
        deadline,
        bindings: BTreeMap::new(),
        children: 0,
        groups: vec![],
    };
    if root.kind() != "program" {
        return Err(UNSUPPORTED);
    }
    let statements = reader.parts(root)?;
    for (index, statement) in statements.iter().enumerate() {
        if index + 1 < statements.len() && !text(*statement, source).ends_with(';') {
            return Err(UNSUPPORTED);
        }
        match statement.kind() {
            "lexical_declaration" => reader.declaration(*statement)?,
            "expression_statement" => {
                let parts = reader.parts(*statement)?;
                let [value] = parts.as_slice() else {
                    return Err(UNSUPPORTED);
                };
                reader.awaited(*value)?;
            }
            _ => return Err(UNSUPPORTED),
        }
    }
    Ok(reader.groups)
}

impl Reader<'_> {
    fn parts<'tree>(&self, node: Node<'tree>) -> Result<Vec<Node<'tree>>> {
        let mut parts = vec![];
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.deadline.remaining()?;
            if child.kind() == "comment" {
                continue;
            }
            if parts.len() >= 32 {
                return Err(OrchestrationFallback::ResourceLimit);
            }
            parts.push(child);
        }
        Ok(parts)
    }

    fn punctuation(&self, node: Node<'_>, allowed: &[&str]) -> Result<()> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.deadline.remaining()?;
            if !child.is_named() && !allowed.contains(&child.kind()) {
                return Err(UNSUPPORTED);
            }
        }
        Ok(())
    }

    fn declaration(&mut self, node: Node<'_>) -> Result<()> {
        if self.bindings.len() >= MAX_VALUES {
            return Err(OrchestrationFallback::ResourceLimit);
        }
        if node
            .child_by_field_name("kind")
            .is_none_or(|n| n.kind() != "const")
        {
            return Err(UNSUPPORTED);
        }
        self.punctuation(node, &["const", ";"])?;
        let parts = self.parts(node)?;
        let [declaration] = parts.as_slice() else {
            return Err(UNSUPPORTED);
        };
        if declaration.kind() != "variable_declarator" || self.parts(*declaration)?.len() != 2 {
            return Err(UNSUPPORTED);
        }
        self.punctuation(*declaration, &["="])?;
        let name = declaration.child_by_field_name("name").ok_or(UNSUPPORTED)?;
        let value = declaration
            .child_by_field_name("value")
            .ok_or(UNSUPPORTED)?;
        let key = self.identifier(name)?;
        if reserved_binding(key) || self.bindings.contains_key(key) {
            return Err(UNSUPPORTED);
        }
        let key = key.to_owned();
        let binding = if value.kind() == "await_expression" {
            self.awaited(value)?;
            Binding::OpaqueResult
        } else {
            Binding::Text(self.string(value)?)
        };
        self.bindings.insert(key, binding);
        Ok(())
    }

    fn identifier(&self, node: Node<'_>) -> Result<&str> {
        let name = text(node, self.source);
        if node.kind() != "identifier" || !name.is_ascii() || name.contains('\\') {
            return Err(UNSUPPORTED);
        }
        Ok(name)
    }

    fn string(&self, node: Node<'_>) -> Result<String> {
        match node.kind() {
            "string" => strings::decode(text(node, self.source), self.deadline),
            "identifier" => match self.bindings.get(self.identifier(node)?) {
                Some(Binding::Text(value)) => Ok(value.clone()),
                _ => Err(UNSUPPORTED),
            },
            _ => Err(UNSUPPORTED),
        }
    }

    fn awaited(&mut self, node: Node<'_>) -> Result<()> {
        if self.groups.len() >= MAX_VALUES {
            return Err(OrchestrationFallback::ResourceLimit);
        }
        if node.kind() != "await_expression" {
            return Err(UNSUPPORTED);
        }
        self.punctuation(node, &["await"])?;
        let parts = self.parts(node)?;
        let [call] = parts.as_slice() else {
            return Err(UNSUPPORTED);
        };
        let (receiver, method, arguments) = self.call_parts(*call)?;
        let (join, children) = match (receiver, method) {
            ("tools", "exec_command") => (OrchestrationJoin::Single, vec![self.child(*call)?]),
            ("Promise", "all" | "allSettled") => {
                let args = self.sequence(arguments, "(", ")")?;
                let [array] = args.as_slice() else {
                    return Err(UNSUPPORTED);
                };
                if array.kind() != "array" {
                    return Err(UNSUPPORTED);
                }
                let join = if method == "all" {
                    OrchestrationJoin::All
                } else {
                    OrchestrationJoin::AllSettled
                };
                let calls = self.sequence(*array, "[", "]")?;
                let mut children = Vec::new();
                for call in calls {
                    children.push(self.child(call)?);
                }
                (join, children)
            }
            _ => return Err(UNSUPPORTED),
        };
        self.groups.push(Group {
            join,
            span: crate::span(node, SourceId(0)),
            children,
        });
        Ok(())
    }

    fn call_parts<'tree>(&self, node: Node<'tree>) -> Result<(&str, &str, Node<'tree>)> {
        if node.kind() != "call_expression" || self.parts(node)?.len() != 2 {
            return Err(UNSUPPORTED);
        }
        self.punctuation(node, &[])?;
        let function = node.child_by_field_name("function").ok_or(UNSUPPORTED)?;
        let arguments = node.child_by_field_name("arguments").ok_or(UNSUPPORTED)?;
        if function.kind() != "member_expression"
            || arguments.kind() != "arguments"
            || self.parts(function)?.len() != 2
        {
            return Err(UNSUPPORTED);
        }
        self.punctuation(function, &["."])?;
        let object = function.child_by_field_name("object").ok_or(UNSUPPORTED)?;
        let property = function
            .child_by_field_name("property")
            .ok_or(UNSUPPORTED)?;
        if property.kind() != "property_identifier" {
            return Err(UNSUPPORTED);
        }
        Ok((
            self.identifier(object)?,
            text(property, self.source),
            arguments,
        ))
    }

    fn child(&mut self, node: Node<'_>) -> Result<Child> {
        if self.children >= MAX_VALUES {
            return Err(OrchestrationFallback::ResourceLimit);
        }
        let (receiver, method, arguments) = self.call_parts(node)?;
        if receiver != "tools" || method != "exec_command" {
            return Err(UNSUPPORTED);
        }
        let args = self.sequence(arguments, "(", ")")?;
        let [object] = args.as_slice() else {
            return Err(UNSUPPORTED);
        };
        if object.kind() != "object" {
            return Err(UNSUPPORTED);
        }
        let fields = self.sequence(*object, "{", "}")?;
        let [pair] = fields.as_slice() else {
            return Err(UNSUPPORTED);
        };
        if pair.kind() != "pair" || self.parts(*pair)?.len() != 2 {
            return Err(UNSUPPORTED);
        }
        self.punctuation(*pair, &[":"])?;
        let key = pair.child_by_field_name("key").ok_or(UNSUPPORTED)?;
        let value = pair.child_by_field_name("value").ok_or(UNSUPPORTED)?;
        if key.kind() != "property_identifier" || text(key, self.source) != "cmd" {
            return Err(UNSUPPORTED);
        }
        let command = self.string(value)?;
        let child = Child {
            id: OrchestrationChildId(self.children),
            call_span: crate::span(node, SourceId(0)),
            command_span: crate::span(value, SourceId(0)),
            command,
        };
        self.children += 1;
        Ok(child)
    }

    fn sequence<'tree>(
        &self,
        node: Node<'tree>,
        open: &str,
        close: &str,
    ) -> Result<Vec<Node<'tree>>> {
        let mut cursor = node.walk();
        let mut children = node
            .children(&mut cursor)
            .filter(|child| child.kind() != "comment");
        if children.next().is_none_or(|child| child.kind() != open) {
            return Err(UNSUPPORTED);
        }
        let mut result = vec![];
        let mut expecting_value = true;
        let mut closed = false;
        for child in children {
            self.deadline.remaining()?;
            if closed {
                return Err(UNSUPPORTED);
            }
            if !child.is_named() && child.kind() == close {
                if expecting_value && !result.is_empty() {
                    return Err(UNSUPPORTED);
                }
                closed = true;
            } else if expecting_value && child.is_named() {
                if result.len() >= MAX_VALUES {
                    return Err(OrchestrationFallback::ResourceLimit);
                }
                result.push(child);
                expecting_value = false;
            } else if !expecting_value && child.kind() == "," {
                expecting_value = true;
            } else {
                return Err(UNSUPPORTED);
            }
        }
        if closed { Ok(result) } else { Err(UNSUPPORTED) }
    }
}
