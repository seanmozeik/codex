//! Literal synchronous array callbacks. Opaque/scheduled callbacks remain gaps.
use super::State;
use crate::children;
use crate::text;
use crate::value::Value;
use std::collections::BTreeMap;
use tree_sitter::Node;

impl<'tree> State<'_, 'tree> {
    pub(super) fn callback(
        &mut self,
        node: Node<'tree>,
        function: &Value,
        args: &[Value],
        depth: usize,
    ) -> Option<Value> {
        let Value::Member(receiver, method) = function else {
            return None;
        };
        if !matches!(method.as_str(), "forEach" | "map")
            || self.language != crate::Language::TypeScript
        {
            return None;
        }
        let Value::Array(values) = receiver.as_ref() else {
            return None;
        };
        let Some(Value::Function(_)) = args.first() else {
            return None;
        };
        // Immutable abstract arrays do not model shared alias mutation. Only
        // inspect callbacks whose syntax cannot mutate their receiver/bindings.
        if values.len() > 128 || args.len() != 1 || self.callback_mutates(&args[0]) {
            self.gap(
                node,
                "Callback mutation or iteration bounds are not inspected",
            );
            self.bindings.clear();
            return Some(Value::Unknown);
        }
        let mut results = Vec::new();
        for value in values {
            let callback_args = [value.clone(), Value::Data, Value::Unknown];
            let result = self.invoke_callback(node, args[0].clone(), &callback_args, depth + 1);
            results.push(result);
        }
        Some(if method == "map" {
            Value::Array(results)
        } else {
            Value::Data
        })
    }

    fn invoke_callback(
        &mut self,
        node: Node<'tree>,
        function: Value,
        args: &[Value],
        depth: usize,
    ) -> Value {
        let count = match &function {
            Value::Function(id) => self.functions.get(id.0).map_or(0, |f| f.parameters.len()),
            _ => 0,
        };
        self.invoke(
            node,
            function,
            &args[..count.min(args.len())],
            &BTreeMap::new(),
            depth,
        )
    }

    fn callback_mutates(&self, value: &Value) -> bool {
        let Value::Function(id) = value else {
            return true;
        };
        let Some(function) = self.functions.get(id.0) else {
            return true;
        };
        let mut pending = vec![function.body];
        let mut visited = 0;
        while let Some(node) = pending.pop() {
            visited += 1;
            if visited > 4096 {
                return true;
            }
            if matches!(
                node.kind(),
                "assignment_expression"
                    | "augmented_assignment_expression"
                    | "update_expression"
                    | "delete_expression"
            ) {
                return true;
            }
            if matches!(node.kind(), "call_expression")
                && let Some(function) = node.child_by_field_name("function")
            {
                // Only direct recognised I/O or local closures without mutations
                // are proven later by invocation. Unknown callees may mutate aliases.
                let name = text(function, self.source);
                if !name.ends_with("writeFileSync")
                    && !name.ends_with("readFileSync")
                    && !name.ends_with("unlinkSync")
                {
                    return true;
                }
            }
            pending.extend(children(node));
        }
        false
    }
}
