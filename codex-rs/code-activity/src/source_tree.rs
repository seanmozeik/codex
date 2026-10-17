//! Shared CST spans and source access.

use crate::SourceId;
use crate::Span;
use tree_sitter::Node;

pub(crate) fn span(node: Node<'_>, source_id: SourceId) -> Span {
    Span {
        source_id,
        start_byte: node.start_byte(),
        end_byte: node.end_byte(),
        start_line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
        start_column: node.start_position().column + 1,
    }
}
pub(crate) fn children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}
pub(crate) fn text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    &source[node.byte_range()]
}
