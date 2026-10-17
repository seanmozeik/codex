//! Finite expression compatibility; admitted expressions are not evaluated.

use super::super::Result;
use super::super::UNSUPPORTED;
use super::Syntax;
use std::collections::BTreeSet;
use tree_sitter::Node;

impl Syntax<'_> {
    pub(super) fn expression(&self, node: Node<'_>) -> Result<()> {
        self.budget.check()?;
        match node.kind() {
            "identifier" => {
                self.identifier(node)?;
            }
            "string" | "template_string" => {
                self.literal_text(node)?;
            }
            "number" => self.decimal(node)?,
            "true" | "false" | "null" if node.child_count() == 0 => {}
            "parenthesized_expression" => self.expression(self.parenthesized(node)?)?,
            "array" => {
                for value in self.sequence(node, "[", "]")? {
                    self.expression(value)?;
                }
            }
            "object" => {
                let mut names = BTreeSet::new();
                for pair in self.sequence(node, "{", "}")? {
                    if pair.kind() == "shorthand_property_identifier" {
                        let name = self.identifier(pair)?;
                        if !names.insert(name.to_owned()) {
                            return Err(UNSUPPORTED);
                        }
                        continue;
                    }
                    let (name, value) = self.pair(pair)?;
                    if !names.insert(name) {
                        return Err(UNSUPPORTED);
                    }
                    self.expression(value)?;
                }
            }
            "member_expression" => {
                let (object, _) = self.member(node)?;
                self.expression(object)?;
            }
            "call_expression" => {
                let (callee, arguments) = self.call(node)?;
                // The suffix extension admits subscript VALUE reads, not
                // computed dispatch. Preserve the frozen whole-module grammar
                // exclusion for direct and parenthesized computed callees.
                if self.peel(callee)?.kind() == "subscript_expression" {
                    return Err(UNSUPPORTED);
                }
                self.expression(callee)?;
                for argument in arguments {
                    self.expression(argument)?;
                }
            }
            "await_expression" => {
                self.punctuation(node, &["await"])?;
                let children = self.parts(node)?;
                let [value] = children.as_slice() else {
                    return Err(UNSUPPORTED);
                };
                let value = self.peel(*value)?;
                if value.kind() != "call_expression" {
                    return Err(UNSUPPORTED);
                }
                self.expression(value)?;
            }
            "binary_expression" => {
                // Compilation-compatible shape only. Reader deliberately does
                // not evaluate, mark pure, or traverse effects in these forms.
                let parts = self.parts(node)?;
                let [left, right] = parts.as_slice() else {
                    return Err(UNSUPPORTED);
                };
                let operator = node.child_by_field_name("operator").ok_or(UNSUPPORTED)?;
                if node.child_by_field_name("left") != Some(*left)
                    || node.child_by_field_name("right") != Some(*right)
                    || operator.is_named()
                    || operator.child_count() != 0
                    || !matches!(operator.kind(), "+" | "??" | "<" | "===")
                    || crate::text(operator, self.source) != operator.kind()
                {
                    return Err(UNSUPPORTED);
                }
                let mut cursor = node.walk();
                let mut children = node
                    .children(&mut cursor)
                    .filter(|child| child.kind() != "comment");
                if children.next() != Some(*left)
                    || children.next() != Some(operator)
                    || children.next() != Some(*right)
                    || children.next().is_some()
                {
                    return Err(UNSUPPORTED);
                }
                self.expression(*left)?;
                self.expression(*right)?;
            }
            "subscript_expression" => {
                self.punctuation(node, &["[", "]"])?;
                let parts = self.parts(node)?;
                let [object, index] = parts.as_slice() else {
                    return Err(UNSUPPORTED);
                };
                if node.child_by_field_name("object") != Some(*object)
                    || node.child_by_field_name("index") != Some(*index)
                {
                    return Err(UNSUPPORTED);
                }
                self.expression(*object)?;
                self.expression(*index)?;
            }
            "update_expression" => {
                self.punctuation(node, &["++"])?;
                let parts = self.parts(node)?;
                let [argument] = parts.as_slice() else {
                    return Err(UNSUPPORTED);
                };
                if node.child_by_field_name("argument") != Some(*argument)
                    || matches!(self.identifier(*argument)?, "eval" | "arguments")
                {
                    return Err(UNSUPPORTED);
                }
                let operator = node.child_by_field_name("operator").ok_or(UNSUPPORTED)?;
                if operator.is_named()
                    || operator.child_count() != 0
                    || crate::text(operator, self.source) != "++"
                {
                    return Err(UNSUPPORTED);
                }
                let mut cursor = node.walk();
                let mut children = node
                    .children(&mut cursor)
                    .filter(|child| child.kind() != "comment");
                let first = children.next().ok_or(UNSUPPORTED)?;
                let second = children.next().ok_or(UNSUPPORTED)?;
                if children.next().is_some()
                    || !((first == *argument && second == operator)
                        || (first == operator && second == *argument))
                {
                    return Err(UNSUPPORTED);
                }
                if first == *argument {
                    let gap = self
                        .source
                        .get(argument.end_byte()..operator.start_byte())
                        .ok_or(UNSUPPORTED)?;
                    // Postfix updates forbid line terminators, even in comments.
                    if gap.contains(['\n', '\r', '\u{2028}', '\u{2029}']) {
                        return Err(UNSUPPORTED);
                    }
                }
            }
            "assignment_expression" => {
                self.punctuation(node, &["="])?;
                let children = self.parts(node)?;
                let [left, right] = children.as_slice() else {
                    return Err(UNSUPPORTED);
                };
                if node.child_by_field_name("left") != Some(*left)
                    || node.child_by_field_name("right") != Some(*right)
                {
                    return Err(UNSUPPORTED);
                }
                match left.kind() {
                    "identifier" if !matches!(self.identifier(*left)?, "eval" | "arguments") => {}
                    "member_expression" => self.expression(*left)?,
                    _ => return Err(UNSUPPORTED),
                }
                self.expression(*right)?;
            }
            _ => return Err(UNSUPPORTED),
        }
        Ok(())
    }
}
