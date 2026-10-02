//! Bounded branch and iteration analysis. Collected effects remain conditional intent.
use super::State;
use super::functions::Flow;
use super::scopes::Bindings;
use crate::children;
use crate::text;
use crate::value::Value;
use tree_sitter::Node;

impl<'tree> State<'_, 'tree> {
    pub(super) fn eval_branches(&mut self, node: Node<'tree>, depth: usize) -> Value {
        self.gap(
            node,
            "Branch effects are conditional on runtime control flow",
        );
        self.branch(node, depth);
        Value::Unknown
    }

    fn branch(&mut self, node: Node<'tree>, depth: usize) {
        if depth > crate::MAX_NESTING {
            self.gap(node, "Branch depth exceeded");
            self.bindings.clear();
            return;
        }
        if let Some(condition) = node.child_by_field_name("condition") {
            self.eval(condition, depth + 1);
        }
        // Both outcomes inherit the condition's side effects.
        let fallthrough = self.bindings.clone();
        let mut flows = vec![Flow::Continue];
        let mut merged = fallthrough.clone();
        if let Some(body) = node
            .child_by_field_name("consequence")
            .or_else(|| node.child_by_field_name("body"))
        {
            self.eval(body, depth + 1);
            merge_into(&mut merged, &self.bindings);
            flows.push(std::mem::replace(&mut self.flow, Flow::Continue));
        }
        self.bindings.restore(&fallthrough);
        for alternative in children(node)
            .into_iter()
            .filter(|n| matches!(n.kind(), "elif_clause" | "else_clause"))
        {
            self.branch(alternative, depth + 1);
            merge_into(&mut merged, &self.bindings);
            flows.push(std::mem::replace(&mut self.flow, Flow::Continue));
        }
        if let Some(alternative) = node.child_by_field_name("alternative")
            && !matches!(alternative.kind(), "else_clause" | "elif_clause")
        {
            self.eval(alternative, depth + 1);
            merge_into(&mut merged, &self.bindings);
            flows.push(std::mem::replace(&mut self.flow, Flow::Continue));
        }
        self.bindings.restore(&merged);
        if flows.iter().any(|flow| !matches!(flow, Flow::Continue)) {
            self.flow = Flow::ConditionalReturn;
            self.gap(
                node,
                "Conditional return prevents exact continuation analysis",
            );
        }
    }

    pub(super) fn eval_loop(&mut self, node: Node<'tree>, depth: usize) -> Value {
        let (Some(target), Some(input), Some(body)) = (
            node.child_by_field_name("left"),
            node.child_by_field_name("right"),
            node.child_by_field_name("body"),
        ) else {
            self.gap(node, "Unsupported loop shape");
            self.bindings.clear();
            return Value::Unknown;
        };
        if node.kind() == "for_in_statement"
            && node
                .child_by_field_name("operator")
                .is_some_and(|op| text(op, self.source) != "of")
        {
            self.gap(node, "Only for-of iteration is inspected");
            self.bindings.clear();
            return Value::Unknown;
        }
        self.iterate(target, input, body, depth, |state| {
            state.eval(body, depth + 1);
        });
        if let Some(alternative) = node.child_by_field_name("alternative") {
            self.branch(alternative, depth + 1);
        }
        Value::Unknown
    }

    pub(super) fn eval_generator(&mut self, node: Node<'tree>, depth: usize) -> Value {
        // Python evaluates the outer iterable when creating a generator, but
        // defers its body, filters and subsequent iterables until consumption.
        if let Some(clause) = children(node)
            .into_iter()
            .find(|n| n.kind() == "for_in_clause")
            && let Some(input) = clause.child_by_field_name("right")
        {
            self.eval(input, depth + 1);
        }
        self.gap(
            node,
            "Lazy generator body and consumption are not inspected",
        );
        Value::Unknown
    }

    pub(super) fn eval_comprehension(&mut self, node: Node<'tree>, depth: usize) -> Value {
        let clauses: Vec<_> = children(node)
            .into_iter()
            .filter(|n| n.kind() == "for_in_clause")
            .collect();
        if clauses.len() != 1 {
            self.gap(node, "Only single-generator comprehensions are inspected");
            self.bindings.clear();
            return Value::Unknown;
        }
        let clause = clauses[0];
        let (Some(target), Some(input), Some(body)) = (
            clause.child_by_field_name("left"),
            clause.child_by_field_name("right"),
            node.child_by_field_name("body"),
        ) else {
            self.gap(node, "Unsupported comprehension shape");
            self.bindings.clear();
            return Value::Unknown;
        };
        let mut before = self.bindings.clone();
        self.iterate(target, input, body, depth, |state| {
            for filter in children(node)
                .into_iter()
                .filter(|n| n.kind() == "if_clause")
            {
                for condition in children(filter) {
                    state.eval(condition, depth + 1);
                }
            }
            state.eval(body, depth + 1);
        });
        // Retain only bindings that agree across the outer and inspected scopes.
        // Unknown code in a comprehension must not resurrect an old path or API.
        merge_into(&mut before, &self.bindings);
        self.bindings.restore(&before);
        Value::Data
    }

    fn iterate(
        &mut self,
        target: Node<'tree>,
        input: Node<'tree>,
        body: Node<'tree>,
        depth: usize,
        mut visit: impl FnMut(&mut Self),
    ) {
        if target.kind() != "identifier" {
            self.gap(target, "Iteration binding is unresolved");
            self.bindings.clear();
            return;
        }
        let iterable = self.eval(input, depth + 1);
        let dynamic = matches!(iterable, Value::Data);
        let values = match iterable {
            Value::Array(values) if values.len() <= 128 => values,
            Value::Data => vec![Value::Data],
            _ => {
                self.gap(input, "Iteration requires bounded literals or inert data");
                self.bindings.clear();
                return;
            }
        };
        self.gap(body, "Loop effects are conditional on runtime iteration");
        let mut merged = self.bindings.clone();
        if dynamic {
            self.forget_writes(body, depth + 1);
        }
        let values = if values.is_empty() {
            vec![Value::Unknown]
        } else {
            values
        };
        for value in values {
            if self.visited > 50_000 {
                self.gap(body, "Iteration budget exceeded");
                self.bindings.clear();
                return;
            }
            self.bind(target, value);
            visit(self);
            if !matches!(self.flow, Flow::Continue) {
                self.flow = Flow::ConditionalReturn;
                self.gap(
                    body,
                    "Return inside a loop prevents exact continuation analysis",
                );
                break;
            }
            merge_into(&mut merged, &self.bindings);
        }
        self.bindings.restore(&merged);
    }

    fn forget_writes(&mut self, node: Node<'tree>, depth: usize) {
        if depth > crate::MAX_NESTING {
            self.bindings.clear();
            return;
        }
        if matches!(
            node.kind(),
            "assignment"
                | "assignment_expression"
                | "variable_declarator"
                | "for_statement"
                | "for_in_statement"
        ) {
            if let Some(target) = node
                .child_by_field_name("left")
                .or_else(|| node.child_by_field_name("name"))
            {
                self.bind(target, Value::Unknown);
            }
        } else if matches!(
            node.kind(),
            "import_statement" | "import_from_statement" | "with_statement"
        ) {
            self.bindings.clear();
        }
        for child in children(node) {
            self.forget_writes(child, depth + 1);
        }
    }
}

fn merge_into(merged: &mut Bindings, state: &Bindings) {
    merged.join(state);
}
