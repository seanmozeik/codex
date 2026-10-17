//! Strict Python loops over a directly written, nonempty sequence of text literals.
use super::State;
use crate::Language;
use crate::MAX_NESTING;
use crate::children;
use crate::text;
use crate::value::HandleMode;
use crate::value::Value;
use crate::value::string_literal;
use std::collections::BTreeMap;
use tree_sitter::Node;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum CallContext {
    Outside,
    LiteralLoop,
}

/// Read-type provenance is erased by existing Handle/Content values. A whole-source
/// rule covers origins before the loop as well as aliases used inside it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SourcePolicy {
    Existing,
    TextOnly,
}

impl SourcePolicy {
    pub(super) fn inspect(root: Node<'_>, mode: super::Mode, language: Language) -> Self {
        if mode != super::Mode::Complete || language != Language::Python {
            return Self::Existing;
        }
        // The parsed source has already passed the source-byte ceiling. Walk
        // with constant auxiliary space; ordinary whole-analysis deadlines still apply.
        let mut cursor = root.walk();
        loop {
            if matches!(cursor.node().kind(), "for_statement" | "for_in_clause") {
                return Self::TextOnly;
            }
            if cursor.goto_first_child() {
                continue;
            }
            while !cursor.goto_next_sibling() {
                if !cursor.goto_parent() {
                    return Self::Existing;
                }
            }
        }
    }
}

impl<'tree> State<'_, 'tree> {
    pub(super) fn reject_loop_source_binary_open(
        &mut self,
        node: Node<'_>,
        mode: Option<&Value>,
    ) -> bool {
        if self.loop_source != SourcePolicy::TextOnly
            || !matches!(mode, Some(Value::Text(mode)) if mode.contains('b'))
        {
            return false;
        }
        self.gap(
            node,
            "Binary open is outside strict Python loop-source semantics",
        );
        self.bindings.clear();
        self.cwd = None;
        true
    }

    pub(super) fn literal_loop_source_structure(
        &mut self,
        node: Node<'tree>,
        depth: usize,
    ) -> bool {
        if node.kind() == "module" {
            return children(node)
                .into_iter()
                .all(|child| self.literal_loop_source_structure(child, depth + 1));
        }
        if node.kind() == "for_statement" {
            return node
                .child_by_field_name("body")
                .is_some_and(|body| self.literal_loop_body(body, depth + 1));
        }
        self.literal_loop_body(node, depth)
    }

    pub(super) fn eval_literal_loop(&mut self, node: Node<'tree>, depth: usize) -> Value {
        let (Some(target), Some(input), Some(body)) = (
            node.child_by_field_name("left"),
            node.child_by_field_name("right"),
            node.child_by_field_name("body"),
        ) else {
            return self.reject_literal_loop(node);
        };
        if self.loop_context != CallContext::Outside
            || target.kind() != "identifier"
            || !matches!(input.kind(), "list" | "tuple")
            || node.child_by_field_name("alternative").is_some()
            || node
                .children(&mut node.walk())
                .any(|child| child.kind() == "async")
            || !self.literal_loop_body(body, depth + 1)
        {
            return self.reject_literal_loop(node);
        }
        let entries: Vec<_> = children(input)
            .into_iter()
            .filter(|entry| entry.kind() != "comment")
            .collect();
        if entries.is_empty() || entries.len() > 16 {
            return self.reject_literal_loop(input);
        }
        let mut values = Vec::with_capacity(entries.len());
        let mut remaining = crate::value::MAX_VALUE_BYTES - 64;
        for entry in entries {
            if entry.kind() != "string" {
                return self.reject_literal_loop(entry);
            }
            let Some(value) = string_literal(text(entry, self.source), Language::Python) else {
                return self.reject_literal_loop(entry);
            };
            let value = Value::Text(value);
            if !self.reserve_value(entry, &value, 0, &mut remaining) {
                return self.reject_literal_loop(entry);
            }
            values.push(value);
        }
        // Never recover eligibility or stale bindings after an earlier source gap.
        if !self.report.unresolved.is_empty() {
            return self.reject_literal_loop(node);
        }
        self.loop_context = CallContext::LiteralLoop;
        for value in values {
            self.bind(target, value);
            self.eval(body, depth + 1);
            if !self.report.unresolved.is_empty() {
                self.bindings.clear();
                self.cwd = None;
                break;
            }
        }
        self.loop_context = CallContext::Outside;
        // The final body state, including rebinding the target, survives the loop.
        Value::Unknown
    }

    fn reject_literal_loop(&mut self, node: Node<'_>) -> Value {
        self.gap(
            node,
            "Strict loop requires a bounded literal sequence and straight-line body",
        );
        self.bindings.clear();
        self.cwd = None;
        Value::Unknown
    }

    fn literal_loop_body(&mut self, node: Node<'tree>, depth: usize) -> bool {
        self.visited += 1;
        if depth > MAX_NESTING || self.visited > 50_000 {
            return false;
        }
        match node.kind() {
            "string" => return string_literal(text(node, self.source), Language::Python).is_some(),
            "assignment" => {
                if node
                    .child_by_field_name("left")
                    .is_none_or(|left| left.kind() != "identifier")
                    || node.child_by_field_name("type").is_some()
                    || node
                        .child_by_field_name("right")
                        .is_none_or(|right| right.kind() == "assignment")
                {
                    return false;
                }
            }
            "binary_operator" => {
                if node
                    .child_by_field_name("operator")
                    .is_none_or(|op| text(op, self.source) != "+")
                {
                    return false;
                }
            }
            "block"
            | "expression_statement"
            | "identifier"
            | "attribute"
            | "call"
            | "argument_list"
            | "keyword_argument"
            | "parenthesized_expression"
            | "integer"
            | "float"
            | "none"
            | "true"
            | "false"
            | "comment" => {}
            _ => return false,
        }
        children(node)
            .into_iter()
            .all(|child| self.literal_loop_body(child, depth + 1))
    }

    pub(super) fn reject_unknown_loop_identifier(&mut self, node: Node<'_>, value: &Value) {
        if self.loop_source == SourcePolicy::TextOnly && matches!(value, Value::Unknown) {
            self.gap(
                node,
                "Unresolved identifier in a strict literal-loop source",
            );
            self.bindings.clear();
            self.cwd = None;
        }
    }

    pub(super) fn literal_loop_binary(
        &mut self,
        node: Node<'_>,
        a: &Value,
        b: &Value,
        operator: &str,
    ) -> bool {
        let supported = match (a, b) {
            (Value::Text(_), Value::Text(_)) => true,
            (Value::Text(_), Value::Content { transforms, .. })
            | (Value::Content { transforms, .. }, Value::Text(_)) => transforms.len() < 32,
            (
                Value::Content {
                    target: a,
                    transforms,
                    ..
                },
                Value::Content { target: b, .. },
            ) => a == b && transforms.len() < 32,
            _ => false,
        };
        self.literal_loop_proof(node, supported && operator == "+")
    }

    pub(super) fn literal_loop_member(&mut self, node: Node<'_>, base: &Value, name: &str) -> bool {
        let supported = matches!(
            (base, name),
            (Value::Text(_) | Value::Content { .. }, "replace")
                | (Value::Content { .. }, "strip" | "lower" | "upper")
                | (
                    Value::Handle {
                        mode: HandleMode::Read,
                        ..
                    },
                    "read",
                )
                | (
                    Value::Handle {
                        mode: HandleMode::Write(_),
                        ..
                    },
                    "write",
                )
        );
        self.literal_loop_proof(node, supported)
    }

    fn literal_loop_proof(&mut self, node: Node<'_>, supported: bool) -> bool {
        if self.loop_source == SourcePolicy::Existing || supported {
            return true;
        }
        self.gap(
            node,
            "Loop expression requires proven text or file-handle semantics",
        );
        self.bindings.clear();
        self.cwd = None;
        false
    }

    /// Enforce the loop's callable boundary before local functions or callbacks run.
    pub(super) fn literal_loop_call(
        &mut self,
        node: Node<'_>,
        function: &Value,
        args: &[Value],
        keywords: &BTreeMap<String, Value>,
    ) -> bool {
        if self.loop_source == SourcePolicy::Existing || admitted_call(function, args, keywords) {
            return true;
        }
        self.gap(
            node,
            "Loop callable or arguments are outside the proven file/text subset",
        );
        self.bindings.clear();
        self.cwd = None;
        false
    }
}

fn admitted_call(function: &Value, args: &[Value], keywords: &BTreeMap<String, Value>) -> bool {
    if matches!(function, Value::Member(receiver, _)
        if matches!(receiver.as_ref(), Value::Content { transforms, .. } if transforms.len() >= 32))
    {
        return false;
    }
    let text_args = args.iter().all(|arg| matches!(arg, Value::Text(_)));
    match function {
        Value::Api(name) if name == "open" => {
            (1..=2).contains(&args.len())
                && text_args
                && !(args.len() == 2 && keywords.contains_key("mode"))
                && keywords.iter().all(|(key, value)| {
                    matches!(key.as_str(), "mode" | "encoding") && matches!(value, Value::Text(_))
                })
        }
        Value::Api(name) if name == "print" => text_args && keywords.is_empty(),
        Value::Member(receiver, method) if keywords.is_empty() => {
            match (receiver.as_ref(), method.as_str()) {
                (
                    Value::Handle {
                        mode: HandleMode::Read,
                        ..
                    },
                    "read",
                ) => args.is_empty(),
                (
                    Value::Handle {
                        mode: HandleMode::Write(_),
                        ..
                    },
                    "write",
                ) => {
                    matches!(args, [Value::Text(_) | Value::Content { .. }])
                }
                (Value::Text(_) | Value::Content { .. }, "replace") => args.len() == 2 && text_args,
                (Value::Content { .. }, "strip" | "lower" | "upper") => args.is_empty(),
                _ => false,
            }
        }
        _ => false,
    }
}
