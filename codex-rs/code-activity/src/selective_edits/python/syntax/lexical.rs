//! Closed lexical gap spelling for the pinned Python grammar.

use crate::SourceDescriptionFallback as Fallback;
use crate::source_description::DescriptionBudget;
use tree_sitter::Node;

pub(super) fn lexical_gaps(
    root: Node<'_>,
    source: &str,
    budget: DescriptionBudget,
) -> Result<(), Fallback> {
    // The pinned grammar accepts Unicode/zero-width extras beyond Python's
    // lexical whitespace. Consume strings/comments as whole tokens, and allow
    // only Python's closed ASCII gap spelling between all other leaf tokens.
    let mut cursor = root.walk();
    let mut end = 0;
    if root.child_count() == 0 {
        return if source
            .bytes()
            .all(|byte| matches!(byte, b' ' | b'\t' | b'\n'))
        {
            Ok(())
        } else {
            Err(Fallback::UnsupportedSource)
        };
    }
    loop {
        budget.check()?;
        let node = cursor.node();
        let atomic = matches!(node.kind(), "string" | "comment" | "line_continuation");
        if node.kind() == "comment" && node.start_position().row < 2 {
            // A byte-input Python carrier can honor an encoding cookie before
            // compiling any body. No non-UTF-8 transport is attested here.
            if crate::text(node, source)
                .as_bytes()
                .windows(7)
                .any(|window| matches!(window, b"coding:" | b"coding="))
            {
                return Err(Fallback::UnsupportedSource);
            }
        }
        if !atomic && cursor.goto_first_child() {
            continue;
        }
        if source[end..node.start_byte()]
            .bytes()
            .any(|byte| !matches!(byte, b' ' | b'\t' | b'\n'))
            || (node.kind() == "line_continuation" && crate::text(node, source) != "\\\n")
        {
            return Err(Fallback::UnsupportedSource);
        }
        if node.kind() == "line_continuation" {
            let before = &source[..node.start_byte()];
            let line_start = before.rfind('\n').map_or(0, |index| index + 1);
            if source[line_start..node.start_byte()]
                .bytes()
                .all(|byte| matches!(byte, b' ' | b'\t'))
            {
                // Logical indentation across a leading explicit continuation is
                // not established by the pinned scanner's accumulated width.
                return Err(Fallback::UnsupportedSource);
            }
        }
        end = node.end_byte();
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return if source[end..]
                    .bytes()
                    .all(|byte| matches!(byte, b' ' | b'\t' | b'\n'))
                {
                    Ok(())
                } else {
                    Err(Fallback::UnsupportedSource)
                };
            }
        }
    }
}
