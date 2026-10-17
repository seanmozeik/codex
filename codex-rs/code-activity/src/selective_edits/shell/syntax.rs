//! Literal shell words, quoted heredocs and exact source ranges.

use super::MAX_ITEMS;
use super::MAX_TEXT;
use super::Part;
use super::Reader;
use super::Result;
use super::UNSUPPORTED;
use crate::SourceDescriptionFallback;
use crate::Span;
use tree_sitter::Node;

impl Reader<'_, '_> {
    pub(super) fn heredoc(&self, node: Node<'_>) -> Result<Span> {
        let parts = self.parts(node)?;
        let parts = match parts.as_slice() {
            [descriptor, rest @ ..] if descriptor.field == Some("descriptor") => {
                let operator = rest.first().ok_or(UNSUPPORTED)?;
                if descriptor.node.kind() != "file_descriptor"
                    || !descriptor.node.is_named()
                    || crate::text(descriptor.node, self.source) != "0"
                    || descriptor.node.end_byte() != operator.node.start_byte()
                {
                    return Err(UNSUPPORTED);
                }
                rest
            }
            parts => parts,
        };
        let (operator, start, body, end) = match parts {
            [operator, start, end] => (operator, start, None, end),
            [operator, start, body, end] if body.node.kind() == "heredoc_body" => {
                (operator, start, Some(body), end)
            }
            _ => return Err(UNSUPPORTED),
        };
        if node.kind() != "heredoc_redirect"
            || operator.node.kind() != "<<"
            || operator.node.is_named()
            || start.node.kind() != "heredoc_start"
            || end.node.kind() != "heredoc_end"
            || parts.iter().any(|part| part.field.is_some())
        {
            return Err(UNSUPPORTED);
        }
        self.horizontal(operator.node.end_byte(), start.node.start_byte())?;
        let delimiter = crate::text(start.node, self.source);
        let quote = delimiter.as_bytes().first().copied().ok_or(UNSUPPORTED)?;
        if !matches!(quote, b'\'' | b'"') || delimiter.as_bytes().last() != Some(&quote) {
            return Err(UNSUPPORTED);
        }
        let marker = delimiter.get(1..delimiter.len() - 1).ok_or(UNSUPPORTED)?;
        if marker.is_empty()
            || !marker
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            || crate::text(end.node, self.source) != marker
            || end.node.start_position().column != 0
            || !matches!(
                self.source.as_bytes().get(end.node.end_byte()),
                None | Some(b'\n')
            )
        {
            return Err(UNSUPPORTED);
        }
        let first_body = body.map_or(end.node.start_byte(), |body| body.node.start_byte());
        let gap = self
            .source
            .get(start.node.end_byte()..first_body)
            .ok_or(UNSUPPORTED)?;
        let newline = start
            .node
            .end_byte()
            .checked_add(gap.find('\n').ok_or(UNSUPPORTED)?)
            .ok_or(UNSUPPORTED)?;
        self.horizontal(start.node.end_byte(), newline)?;
        let physical_start = newline.checked_add(1).ok_or(UNSUPPORTED)?;
        let physical_end = end.node.start_byte();
        if let Some(body) = body {
            if body.node.start_byte() < physical_start || body.node.end_byte() > physical_end {
                return Err(UNSUPPORTED);
            }
            self.trivia(physical_start, body.node.start_byte())?;
            self.trivia(body.node.end_byte(), physical_end)?;
            for part in self.parts(body.node)? {
                if part.node.kind() != "heredoc_content" || part.field.is_some() {
                    return Err(UNSUPPORTED);
                }
            }
        } else {
            self.trivia(physical_start, physical_end)?;
        }
        self.range(physical_start, physical_end)
    }

    pub(super) fn literal(&self, node: Node<'_>) -> Result<String> {
        let mut pending = vec![node];
        while let Some(part) = pending.pop() {
            self.budget.check()?;
            if !matches!(
                part.kind(),
                "word" | "raw_string" | "string" | "string_content" | "concatenation" | "-"
            ) {
                return Err(UNSUPPORTED);
            }
            pending.extend(crate::children(part));
        }
        let value = crate::shell::literals::literal(node, self.source).ok_or(UNSUPPORTED)?;
        if value.len() > MAX_TEXT {
            return Err(SourceDescriptionFallback::SourceLimit);
        }
        Ok(value)
    }

    pub(super) fn parts<'tree>(&self, node: Node<'tree>) -> Result<Vec<Part<'tree>>> {
        let mut result = Vec::new();
        let mut cursor = node.walk();
        for (index, child) in node.children(&mut cursor).enumerate() {
            self.budget.check()?;
            if child.kind() != "comment" {
                if result.len() >= MAX_ITEMS {
                    return Err(SourceDescriptionFallback::SourceLimit);
                }
                result.push(Part {
                    node: child,
                    field: node.field_name_for_child(index as u32),
                });
            }
        }
        Ok(result)
    }

    pub(super) fn horizontal(&self, start: usize, end: usize) -> Result<()> {
        self.budget.check()?;
        if self
            .source
            .get(start..end)
            .ok_or(UNSUPPORTED)?
            .bytes()
            .all(|b| matches!(b, b' ' | b'\t'))
        {
            Ok(())
        } else {
            Err(UNSUPPORTED)
        }
    }

    pub(super) fn trivia(&self, start: usize, end: usize) -> Result<()> {
        self.budget.check()?;
        if self
            .source
            .get(start..end)
            .ok_or(UNSUPPORTED)?
            .bytes()
            .all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c))
        {
            Ok(())
        } else {
            Err(UNSUPPORTED)
        }
    }

    pub(super) fn range(&self, start: usize, end: usize) -> Result<Span> {
        self.source.get(start..end).ok_or(UNSUPPORTED)?;
        let before = self.source.get(..start).ok_or(UNSUPPORTED)?;
        let through = self.source.get(..end).ok_or(UNSUPPORTED)?;
        Ok(Span {
            source_id: self.source_id,
            start_byte: start,
            end_byte: end,
            start_line: before.bytes().filter(|b| *b == b'\n').count() + 1,
            end_line: through.bytes().filter(|b| *b == b'\n').count() + 1,
            start_column: before.rfind('\n').map_or(start + 1, |index| start - index),
        })
    }
}
