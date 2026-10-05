//! Expression evaluation and content-origin propagation.
use super::State;
use super::calls;
use crate::Transform;
use crate::children;
use crate::text;
use crate::value::MAX_VALUE_BYTES;
use crate::value::Value;
use std::collections::BTreeMap;
use tree_sitter::Node;

impl<'tree> State<'_, 'tree> {
    pub(super) fn eval_array(&mut self, node: Node<'tree>, depth: usize) -> Value {
        let mut values = Vec::new();
        let mut remaining = MAX_VALUE_BYTES - 64;
        for child in children(node) {
            let value = self.eval(child, depth + 1);
            if !self.reserve_value(child, &value, /*key_bytes*/ 0, &mut remaining) {
                return Value::Unknown;
            }
            values.push(value);
        }
        Value::Array(values)
    }

    pub(super) fn eval_await(&mut self, node: Node<'tree>, depth: usize) -> Value {
        match node
            .named_child(0)
            .map_or(Value::Unknown, |n| self.eval(n, depth + 1))
        {
            Value::Deferred(value) => *value,
            Value::Unknown => {
                self.gap(node, "Awaited value and its side effects are unresolved");
                self.bindings.clear();
                self.cwd = None;
                Value::Unknown
            }
            other => other,
        }
    }

    pub(super) fn eval_member(&mut self, node: Node<'tree>, depth: usize) -> Value {
        let object = node.child_by_field_name("object");
        let property = node
            .child_by_field_name("attribute")
            .or_else(|| node.child_by_field_name("property"));
        match (object, property) {
            (Some(o), Some(p)) => {
                let base = self.eval(o, depth + 1);
                let name = text(p, self.source).to_owned();
                match base {
                    Value::Api(api) => Value::Api(format!("{api}.{name}")),
                    Value::Object(fields) if self.language == crate::Language::TypeScript => {
                        fields.get(&name).cloned().unwrap_or(Value::Data)
                    }
                    Value::Data if self.language == crate::Language::TypeScript => Value::Data,
                    other => Value::Member(Box::new(other), name),
                }
            }
            _ => Value::Unknown,
        }
    }
    pub(super) fn eval_call(&mut self, node: Node<'tree>, depth: usize) -> Value {
        let function = node
            .child_by_field_name("function")
            .map_or(Value::Unknown, |n| self.eval(n, depth + 1));
        if let Some(arguments) = node.child_by_field_name("arguments")
            && arguments.kind() == "generator_expression"
        {
            let value = self.eval_generator(arguments, depth + 1);
            return calls::evaluate(self, node, function, &[value], &BTreeMap::new());
        }
        let arg_nodes = node
            .child_by_field_name("arguments")
            .map(children)
            .unwrap_or_default();
        let mut args = vec![];
        let mut keywords = BTreeMap::new();
        let mut duplicates = false;
        let mut remaining = MAX_VALUE_BYTES;
        for arg in arg_nodes {
            if arg.kind() == "keyword_argument" {
                if let (Some(name), Some(value)) = (
                    arg.child_by_field_name("name"),
                    arg.child_by_field_name("value"),
                ) {
                    let value = self.eval(value, depth + 1);
                    if !self.reserve_value(
                        arg,
                        &value,
                        text(name, self.source).len(),
                        &mut remaining,
                    ) {
                        return Value::Unknown;
                    }
                    duplicates |= keywords
                        .insert(text(name, self.source).to_owned(), value)
                        .is_some();
                }
            } else {
                let value = self.eval(arg, depth + 1);
                if !self.reserve_value(arg, &value, /*key_bytes*/ 0, &mut remaining) {
                    return Value::Unknown;
                }
                args.push(value);
            }
        }
        if duplicates {
            self.gap(node, "Duplicate keyword argument; call skipped");
            self.bindings.clear();
            return Value::Unknown;
        }
        self.invoke(node, function, &args, &keywords, depth + 1)
    }
    pub(super) fn eval_object(&mut self, node: Node<'tree>, depth: usize) -> Value {
        let mut map = BTreeMap::new();
        let mut complete = true;
        let mut remaining = MAX_VALUE_BYTES - 64;
        for pair in children(node) {
            if pair.kind() == "shorthand_property_identifier" {
                let key = text(pair, self.source).to_owned();
                let value = self.bindings.get(&key).cloned().unwrap_or(Value::Unknown);
                if !self.reserve_value(pair, &value, key.len(), &mut remaining) {
                    return Value::Unknown;
                }
                map.insert(key, value);
                continue;
            }
            if let (Some(k), Some(v)) = (
                pair.child_by_field_name("key"),
                pair.child_by_field_name("value"),
            ) {
                let key = if k.kind() == "property_identifier" {
                    Some(text(k, self.source).to_owned())
                } else {
                    self.eval(k, depth + 1).text()
                };
                let value = self.eval(v, depth + 1);
                if let Some(key) = key {
                    if !self.reserve_value(pair, &value, key.len(), &mut remaining) {
                        return Value::Unknown;
                    }
                    map.insert(key, value);
                } else {
                    self.gap(k, "Computed object key is unresolved; bindings invalidated");
                    self.bindings.clear();
                    self.cwd = None;
                    complete = false;
                }
            } else {
                self.gap(
                    pair,
                    "Computed object property is unresolved; bindings invalidated",
                );
                self.bindings.clear();
                self.cwd = None;
                complete = false;
            }
        }
        if complete {
            Value::Object(map)
        } else {
            Value::Unknown
        }
    }
    pub(super) fn eval_binary(&mut self, node: Node<'tree>, depth: usize) -> Value {
        let Some(left) = node.child_by_field_name("left") else {
            return Value::Unknown;
        };
        let Some(right) = node.child_by_field_name("right") else {
            return Value::Unknown;
        };
        let a = self.eval(left, depth + 1);
        let operator = node.child_by_field_name("operator").map_or_else(
            || self.source[left.end_byte()..right.start_byte()].trim(),
            |operator| text(operator, self.source),
        );
        if matches!(operator, "&&" | "||" | "??" | "and" | "or") {
            match (&a, operator) {
                (Value::Boolean(false), "&&" | "and") | (Value::Boolean(true), "||" | "or") => {
                    return a;
                }
                (Value::Boolean(true), "&&" | "and") | (Value::Boolean(false), "||" | "or") => {
                    return self.eval(right, depth + 1);
                }
                _ => {
                    let before = self.bindings.clone();
                    self.gap(node, "Short-circuit effects depend on runtime control flow");
                    self.eval(right, depth + 1);
                    let mut merged = before;
                    merged.join(&self.bindings);
                    self.bindings.restore(&merged);
                    return Value::Unknown;
                }
            }
        }
        let b = self.eval(right, depth + 1);
        match (a, b, operator) {
            (Value::Path(a), Value::Text(b), "/") => Value::Path(if b.starts_with('/') {
                b
            } else {
                format!("{}/{b}", a.trim_end_matches('/'))
            }),
            (Value::Text(a), Value::Text(b), "+") => Value::Text(format!("{a}{b}")),
            (
                Value::Content {
                    target,
                    mut transforms,
                },
                Value::Text(_),
                "+",
            )
            | (
                Value::Text(_),
                Value::Content {
                    target,
                    mut transforms,
                },
                "+",
            ) => {
                if transforms.len() < 32 {
                    transforms.push(Transform::TextConcatenation);
                }
                Value::Content { target, transforms }
            }
            (
                Value::Content {
                    target: a,
                    mut transforms,
                },
                Value::Content { target: b, .. },
                "+",
            ) if a == b => {
                if transforms.len() < 32 {
                    transforms.push(Transform::TextConcatenation);
                }
                Value::Content {
                    target: a,
                    transforms,
                }
            }
            _ => Value::Unknown,
        }
    }
    pub(super) fn eval_subscript(&mut self, node: Node<'tree>, depth: usize) -> Value {
        let receiver = node
            .child_by_field_name("value")
            .or_else(|| node.child_by_field_name("object"));
        let index = node
            .child_by_field_name("subscript")
            .or_else(|| node.child_by_field_name("index"));
        let value = receiver.map_or(Value::Unknown, |n| self.eval(n, depth + 1));
        if index.is_some_and(|n| n.kind() == "slice") {
            if let Some(index) = index {
                for bound in children(index) {
                    self.eval(bound, depth + 1);
                }
            }
            if let Value::Content {
                target,
                mut transforms,
            } = value
            {
                if transforms.len() < 32 {
                    transforms.push(Transform::TextSlice);
                }
                return Value::Content { target, transforms };
            }
            Value::Data
        } else {
            let key = index.map_or(Value::Unknown, |n| self.eval(n, depth + 1));
            match (value, key) {
                (Value::Object(fields), Value::Text(key)) => {
                    fields.get(&key).cloned().unwrap_or(Value::Data)
                }
                (Value::Array(values), _) => index
                    .and_then(|n| text(n, self.source).parse::<usize>().ok())
                    .and_then(|i| values.get(i).cloned())
                    .unwrap_or(Value::Data),
                (Value::Data | Value::Text(_) | Value::Content { .. }, _) => Value::Data,
                _ => {
                    self.gap(node, "Subscript is unresolved");
                    Value::Unknown
                }
            }
        }
    }
}
