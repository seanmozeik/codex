//! Closed local assignment and single open-context syntax.

use super::MAX_BINDINGS;
use super::Origin;
use super::SourceDescriptionFallback;
use super::State;
use super::identifier;
use super::parts;
use tree_sitter::Node;

enum BindingState {
    Fresh,
    Data,
    Path,
}

impl BindingState {
    fn accepts(&self, origin: &Origin) -> bool {
        match self {
            Self::Fresh => true,
            Self::Data => origin.is_data(),
            Self::Path => matches!(origin, Origin::PathCall(_)),
        }
    }
}

impl State<'_> {
    pub(super) fn statement(&mut self, statement: Node<'_>) -> Option<()> {
        if self.budget.expired() {
            return None;
        }
        if statement.kind() == "with_statement" {
            return self.with_open(statement);
        }
        self.simple_statement(statement)
    }

    fn simple_statement(&mut self, statement: Node<'_>) -> Option<()> {
        if statement.kind() != "expression_statement" {
            return None;
        }
        let items = parts(statement);
        let [expression] = items.as_slice() else {
            return None;
        };
        if expression.kind() != "assignment" {
            let origin = self.eval(*expression, 0)?;
            return (!matches!(origin, Origin::OpenName)).then_some(());
        }
        if expression.child_by_field_name("type").is_some() {
            return None;
        }
        let left = expression.child_by_field_name("left")?;
        let name = crate::text(left, self.source);
        if left.kind() != "identifier" || !self.local_name(name) {
            return None;
        }
        let binding = match self.bindings.get(name) {
            None => BindingState::Fresh,
            Some(Origin::PathCall(_)) => BindingState::Path,
            Some(origin) if origin.is_data() => BindingState::Data,
            Some(_) => return None,
        };
        // Evaluate against the previous binding before replacing it. Aliases own
        // their syntax origins, so updating s never changes an earlier t = s.
        let origin = self.eval(expression.child_by_field_name("right")?, 0)?;
        if matches!(
            origin,
            Origin::DiscardedWrite
                | Origin::DiscardedOutput
                | Origin::ContextOpen { .. }
                | Origin::ClosedContext
        ) || !binding.accepts(&origin)
        {
            return None;
        }
        self.bind(name, origin)
    }

    fn with_open(&mut self, node: Node<'_>) -> Option<()> {
        // Async and multi-context forms have different source/control structure.
        let mut cursor = node.walk();
        if node
            .children(&mut cursor)
            .any(|child| child.kind() == "async")
        {
            return None;
        }
        let items = parts(node);
        let [clause, body] = items.as_slice() else {
            return None;
        };
        if clause.kind() != "with_clause" || body.kind() != "block" {
            return None;
        }
        let contexts = parts(*clause);
        let [item] = contexts.as_slice() else {
            return None;
        };
        if item.kind() != "with_item" {
            return None;
        }
        let value = item.child_by_field_name("value")?;
        if value.kind() != "as_pattern" {
            return None;
        }
        let pattern = parts(value);
        let [call, target] = pattern.as_slice() else {
            return None;
        };
        if call.kind() != "call" || target.kind() != "as_pattern_target" {
            return None;
        }
        let callee = call.child_by_field_name("function")?;
        if callee.kind() != "identifier" || !self.is_open_name(crate::text(callee, self.source)) {
            return None;
        }
        let aliases = parts(*target);
        let [alias] = aliases.as_slice() else {
            return None;
        };
        let name = crate::text(*alias, self.source);
        if alias.kind() != "identifier"
            || !self.local_name(name)
            || self.bindings.contains_key(name)
        {
            return None;
        }
        let Origin::OpenCall { literal, mode } = self.eval(*call, 0)? else {
            return None;
        };
        self.bind(name, Origin::ContextOpen { literal, mode })?;
        for statement in parts(*body) {
            // No nested with/control/function syntax in this initial context slice.
            self.simple_statement(statement)?;
        }
        // Preserve an unusable name marker rather than claiming a live handle
        // after the syntactic context closes. Derived read-data remains usable.
        self.bindings.insert(name.to_owned(), Origin::ClosedContext);
        Some(())
    }

    fn local_name(&self, name: &str) -> bool {
        identifier(name)
            && !matches!(name, "open" | "print")
            && self.constructor.as_deref() != Some(name)
    }

    fn bind(&mut self, name: &str, origin: Origin) -> Option<()> {
        // Accounting is cumulative: reassignment never replenishes the budget.
        self.retain(name.len().checked_add(origin.retained())?)?;
        if !self.bindings.contains_key(name) && self.bindings.len() >= MAX_BINDINGS {
            self.failure = SourceDescriptionFallback::SourceLimit;
            return None;
        }
        self.bindings.insert(name.to_owned(), origin);
        Some(())
    }
}
