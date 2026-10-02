//! Declaration-time Python local-name and generator classification.
use super::State;
use crate::children;
use crate::text;
use crate::value::Value;
use tree_sitter::Node;

pub(super) fn unsupported_python_body(body: Node<'_>) -> bool {
    let mut pending = vec![body];
    let mut visited = 0;
    while let Some(node) = pending.pop() {
        visited += 1;
        if visited > 4096
            || matches!(
                node.kind(),
                "yield"
                    | "global_statement"
                    | "nonlocal_statement"
                    | "class_definition"
                    | "try_statement"
                    | "delete_statement"
                    | "named_expression"
                    | "augmented_assignment"
                    | "wildcard_import"
            )
            || matches!(node.kind(), "assignment" | "for_statement")
                && node
                    .child_by_field_name("left")
                    .is_some_and(|name| name.kind() != "identifier")
            || node.kind() == "as_pattern_target"
                && node
                    .named_child(0)
                    .is_some_and(|name| name.kind() != "identifier")
        {
            return true;
        }
        if !matches!(node.kind(), "function_definition" | "lambda") {
            pending.extend(children(node));
        }
    }
    false
}

impl State<'_, '_> {
    pub(super) fn seed_python_bindings(&mut self, node: Node<'_>) {
        if matches!(node.kind(), "import_statement" | "import_from_statement") {
            for item in children(node) {
                if node.child_by_field_name("module_name") == Some(item) {
                    continue;
                }
                let name = if item.kind() == "aliased_import" {
                    item.child_by_field_name("alias")
                } else {
                    Some(item)
                };
                if let Some(name) = name {
                    let name = text(name, self.source).split('.').next().unwrap_or("");
                    if !name.is_empty() && !self.bindings.local(name) {
                        self.bindings.insert(name.into(), Value::Unknown);
                    }
                }
            }
        } else if node.kind() == "as_pattern_target"
            && let Some(name) = node.named_child(0)
            && name.kind() == "identifier"
            && !self.bindings.local(text(name, self.source))
        {
            self.bindings
                .insert(text(name, self.source).into(), Value::Unknown);
        }
    }
}

pub(super) fn unsupported_js_body(body: Node<'_>) -> bool {
    let mut pending = vec![body];
    let mut visited = 0;
    while let Some(node) = pending.pop() {
        visited += 1;
        if visited > 4096 || node.kind() == "variable_declaration" {
            return true;
        }
        if !matches!(
            node.kind(),
            "function_declaration" | "function_expression" | "arrow_function"
        ) {
            pending.extend(children(node));
        }
    }
    false
}
