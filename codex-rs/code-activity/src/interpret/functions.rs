//! Static calls only; definitions do not imply body execution.
use super::State;
use super::calls;
use super::scopes::ScopeId;
use crate::Language;
use crate::children;
use crate::text;
use crate::value::FunctionId;
use crate::value::Value;
use std::collections::BTreeMap;
use tree_sitter::Node;

#[derive(Clone)]
pub(super) struct Function<'tree> {
    pub(super) body: Node<'tree>,
    pub(super) parameters: Vec<Node<'tree>>,
    scope: ScopeId,
    asynchronous: bool,
    self_name: Option<String>,
}

#[derive(Clone)]
pub(super) enum Flow {
    Continue,
    Return(Value),
    ConditionalReturn,
}

impl<'tree> State<'_, 'tree> {
    pub(super) fn enter_scope(&mut self, node: Node<'tree>, parent: ScopeId) -> bool {
        if self.bindings.enter(parent) {
            return true;
        }
        self.gap(node, "Lexical scope budget exceeded");
        self.bindings.clear();
        false
    }

    pub(super) fn seed_locals(&mut self, node: Node<'tree>, depth: usize, recursive: bool) {
        if depth > crate::MAX_NESTING {
            self.bindings.clear();
            return;
        }
        for child in children(node) {
            let name = match child.kind() {
                "assignment" | "variable_declarator" | "for_statement" => child
                    .child_by_field_name("left")
                    .or_else(|| child.child_by_field_name("name")),
                "function_definition" | "function_declaration" => child.child_by_field_name("name"),
                _ => None,
            };
            if let Some(name) = name
                && name.kind() == "identifier"
            {
                let key = text(name, self.source);
                if !self.bindings.local(key) {
                    self.insert_binding(name, key.into(), Value::Unknown);
                }
            }
            if matches!(
                child.kind(),
                "function_definition"
                    | "function_declaration"
                    | "arrow_function"
                    | "lambda"
                    | "function_expression"
            ) {
                continue;
            }
            if self.language == Language::Python && recursive {
                self.seed_python_bindings(child);
            }
            if recursive || matches!(child.kind(), "lexical_declaration" | "variable_declaration") {
                self.seed_locals(child, depth + 1, recursive);
            }
        }
    }

    pub(super) fn define_function(&mut self, node: Node<'tree>, depth: usize) -> Value {
        let Some(body) = node.child_by_field_name("body") else {
            self.gap(node, "Function body is unresolved");
            return Value::Unknown;
        };
        if self.functions.len() >= 128 {
            self.gap(node, "Function definition budget exceeded");
            return Value::Unknown;
        }
        if self.language == Language::Python
            && super::function_locals::unsupported_python_body(body)
        {
            self.gap(
                node,
                "Python generator, local binding shape or preflight budget is not inspected",
            );
            return Value::Unknown;
        }
        if self.language == Language::TypeScript
            && super::function_locals::unsupported_js_body(body)
        {
            self.gap(
                node,
                "JavaScript var hoisting or function preflight budget is not inspected",
            );
            return Value::Unknown;
        }
        // Generator semantics and decorators require separate execution models.
        if text(node, self.source).starts_with("function*")
            || node.child(0).is_some_and(|token| token.kind() == "async")
                && self.language == Language::Python
        {
            self.gap(node, "Generator or Python async function is not inspected");
            return Value::Unknown;
        }
        let parameters = node
            .child_by_field_name("parameters")
            .map(children)
            .or_else(|| node.child_by_field_name("parameter").map(|p| vec![p]))
            .unwrap_or_default();
        if parameters.iter().any(|p| {
            !matches!(p.kind(), "identifier" | "required_parameter")
                || p.child_by_field_name("value").is_some()
                || p.child_by_field_name("pattern")
                    .is_some_and(|name| name.kind() != "identifier")
        }) {
            self.gap(
                node,
                "Function defaults, destructuring or variadic parameters are not inspected",
            );
            // Python defaults execute when a definition is evaluated.
            if self.language == Language::Python {
                for parameter in &parameters {
                    if let Some(value) = parameter.child_by_field_name("value") {
                        self.eval(value, depth + 1);
                    }
                }
            }
            return Value::Unknown;
        }
        let id = FunctionId(self.functions.len());
        self.functions.push(Function {
            body,
            parameters,
            scope: self.bindings.current(),
            asynchronous: node.child(0).is_some_and(|token| token.kind() == "async"),
            self_name: if node.kind() == "function_expression" {
                node.child_by_field_name("name")
                    .map(|name| text(name, self.source).into())
            } else {
                None
            },
        });
        let value = Value::Function(id);
        if matches!(node.kind(), "function_definition" | "function_declaration")
            && let Some(name) = node.child_by_field_name("name")
        {
            self.bind(name, value.clone());
        }
        value
    }

    pub(super) fn eval_return(&mut self, node: Node<'tree>, depth: usize) -> Value {
        let value = node
            .named_child(0)
            .map_or(Value::Data, |n| self.eval(n, depth + 1));
        if self.active_calls.is_empty() {
            self.gap(node, "Return outside an inspected function");
        } else {
            self.flow = Flow::Return(value.clone());
        }
        value
    }

    pub(super) fn invoke(
        &mut self,
        node: Node<'tree>,
        function: Value,
        args: &[Value],
        keywords: &BTreeMap<String, Value>,
        depth: usize,
    ) -> Value {
        if let Some(value) = self.callback(node, &function, args, depth) {
            return value;
        }
        let Value::Function(id) = function else {
            return calls::evaluate(self, node, function, args, keywords);
        };
        if self.active_calls.contains(&id)
            || self.active_calls.len() >= 16
            || depth > crate::MAX_NESTING
        {
            self.gap(node, "Recursive call or function depth budget exceeded");
            self.bindings.clear();
            return Value::Unknown;
        }
        let Some(function) = self.functions.get(id.0).cloned() else {
            return Value::Unknown;
        };
        let invalid = function
            .parameters
            .iter()
            .enumerate()
            .any(|(index, parameter)| {
                let name = parameter
                    .child_by_field_name("pattern")
                    .unwrap_or(*parameter);
                let key = text(name, self.source);
                args.get(index).is_some() && keywords.contains_key(key)
                    || self.language == Language::Python
                        && args.get(index).is_none()
                        && !keywords.contains_key(key)
            });
        if invalid {
            self.gap(node, "Missing or duplicate function argument; body skipped");
            self.bindings.clear();
            return Value::Unknown;
        }
        let async_before = function.asynchronous.then(|| self.bindings.clone());
        let caller = self.bindings.current();
        if !self.enter_scope(node, function.scope) {
            return Value::Unknown;
        }
        if let Some(name) = &function.self_name {
            self.insert_binding(node, name.clone(), Value::Function(id));
        }
        let mut used = 0;
        for (index, parameter) in function.parameters.iter().enumerate() {
            let name = parameter
                .child_by_field_name("pattern")
                .unwrap_or(*parameter);
            let key = text(name, self.source);
            let positional = args.get(index);
            let keyword = keywords.get(key);
            if positional.is_some() && keyword.is_some() {
                self.gap(node, "Duplicate function argument");
            }
            if keyword.is_some() {
                used += 1;
            }
            self.bind(
                name,
                positional.or(keyword).cloned().unwrap_or(Value::Unknown),
            );
        }
        if used != keywords.len() || args.len() > function.parameters.len() {
            self.gap(node, "Function argument binding is incomplete");
            self.bindings.set_current(caller);
            self.bindings.clear();
            return Value::Unknown;
        }
        self.seed_locals(function.body, depth + 1, self.language == Language::Python);
        let saved_flow = std::mem::replace(&mut self.flow, Flow::Continue);
        self.active_calls.push(id);
        let expression = !matches!(function.body.kind(), "block" | "statement_block");
        let value = self.eval(function.body, depth + 1);
        let result = match std::mem::replace(&mut self.flow, saved_flow) {
            Flow::Return(value) => value,
            Flow::Continue if expression => value,
            Flow::Continue => Value::Data,
            Flow::ConditionalReturn => Value::Unknown,
        };
        self.active_calls.pop();
        self.bindings.set_current(caller);
        if let Some(before) = async_before {
            self.bindings.invalidate_changes(&before);
            self.gap(node, "Async function effects depend on scheduling");
            Value::Deferred(Box::new(result))
        } else {
            result
        }
    }
}
