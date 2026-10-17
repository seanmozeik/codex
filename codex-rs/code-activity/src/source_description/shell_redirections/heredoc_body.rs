use super::DescriptionBudget;
use super::Result;
use super::UNSUPPORTED;
use super::literals;
use tree_sitter::Node;

pub(super) fn physical_body_start(
    node: Node<'_>,
    source: &str,
    header_end: usize,
    first_body_byte: usize,
    budget: DescriptionBudget,
) -> Result<usize> {
    budget.check()?;
    let gap = source.get(header_end..first_body_byte).ok_or(UNSUPPORTED)?;
    let newline = header_end
        .checked_add(gap.find('\n').ok_or(UNSUPPORTED)?)
        .ok_or(UNSUPPORTED)?;
    if source.as_bytes().get(newline.saturating_sub(1)) == Some(&b'\r') {
        return Err(UNSUPPORTED);
    }

    // Only exact direct comment nodes may interrupt ASCII header trivia.
    // No body bytes or other omitted syntax are accepted as header padding.
    let mut consumed = header_end;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        budget.check()?;
        if child.kind() == "comment"
            && child.start_byte() >= header_end
            && child.end_byte() <= newline
        {
            let trivia = source
                .get(consumed..child.start_byte())
                .ok_or(UNSUPPORTED)?;
            if !literals::ascii_trivia(trivia) {
                return Err(UNSUPPORTED);
            }
            consumed = child.end_byte();
        }
    }
    if !literals::ascii_trivia(source.get(consumed..newline).ok_or(UNSUPPORTED)?) {
        return Err(UNSUPPORTED);
    }
    newline.checked_add(1).ok_or(UNSUPPORTED)
}
