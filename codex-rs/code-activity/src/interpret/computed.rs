//! Strict-only literal lookup for four existing synchronous filesystem APIs.

use crate::MAX_NESTING;
use crate::text;
use tree_sitter::Node;

pub(super) fn filesystem_member(
    api: &str,
    key: &str,
    index: Option<Node<'_>>,
    source: &str,
) -> bool {
    api == "fs"
        && matches!(
            key,
            "readFileSync" | "writeFileSync" | "appendFileSync" | "unlinkSync"
        )
        && index.is_some_and(|node| literal_text(node, source, 0))
}

fn literal_text(node: Node<'_>, source: &str, depth: usize) -> bool {
    if depth > MAX_NESTING {
        return false;
    }
    match node.kind() {
        "string" => true,
        "parenthesized_expression" => node
            .named_child(0)
            .is_some_and(|inner| literal_text(inner, source, depth + 1)),
        "binary_expression" => {
            node.child_by_field_name("operator")
                .is_some_and(|operator| text(operator, source) == "+")
                && node
                    .child_by_field_name("left")
                    .is_some_and(|left| literal_text(left, source, depth + 1))
                && node
                    .child_by_field_name("right")
                    .is_some_and(|right| literal_text(right, source, depth + 1))
        }
        _ => false,
    }
}
