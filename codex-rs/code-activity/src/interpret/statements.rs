//! Assignment and context-manager statement evaluation.
use super::State;
use crate::children;
use crate::value::Value;
use tree_sitter::Node;

impl<'tree> State<'_, 'tree> {
    pub(super) fn eval_sequence(&mut self, node: Node<'tree>, depth: usize) -> Value {
        let parent = self.bindings.current();
        let scoped =
            node.kind() == "statement_block" && self.language == crate::Language::TypeScript;
        if scoped && !self.enter_scope(node, parent) {
            return Value::Unknown;
        }
        self.seed_locals(node, depth + 1, /*recursive*/ false);
        let mut value = Value::Unknown;
        for child in children(node) {
            value = self.eval(child, depth + 1);
            if !matches!(self.flow, super::functions::Flow::Continue) {
                break;
            }
        }
        if scoped {
            self.bindings.set_current(parent);
        }
        value
    }

    pub(super) fn eval_assignment(&mut self, node: Node<'tree>, depth: usize) -> Value {
        let lhs = node
            .child_by_field_name("left")
            .or_else(|| node.child_by_field_name("name"));
        let rhs = node
            .child_by_field_name("right")
            .or_else(|| node.child_by_field_name("value"));
        let value = rhs.map_or(Value::Unknown, |n| self.eval(n, depth + 1));
        if let Some(lhs) = lhs {
            if node.kind() == "assignment_expression"
                && self.language == crate::Language::TypeScript
            {
                if !self
                    .bindings
                    .assign(crate::text(lhs, self.source), value.clone())
                {
                    self.gap(
                        lhs,
                        "Abstract value or binding budget exceeded; bindings invalidated",
                    );
                }
            } else {
                self.bind(lhs, value.clone());
            }
        }
        value
    }
    pub(super) fn eval_with(&mut self, node: Node<'tree>, depth: usize) -> Value {
        for clause in children(node)
            .into_iter()
            .filter(|n| n.kind() == "with_clause")
        {
            for item in children(clause) {
                let Some(value) = item.child_by_field_name("value") else {
                    continue;
                };
                if value.kind() == "as_pattern" {
                    let result = value
                        .named_child(0)
                        .map_or(Value::Unknown, |n| self.eval(n, depth + 1));
                    if let Some(alias) = value
                        .child_by_field_name("alias")
                        .and_then(|n| n.named_child(0))
                    {
                        self.bind(alias, result);
                    }
                } else {
                    self.eval(value, depth + 1);
                }
            }
        }
        if let Some(body) = node.child_by_field_name("body") {
            self.eval(body, depth + 1);
        }
        Value::Unknown
    }
}
