//! Exact lexical gaps, finite literals and reserved identifiers.

use super::super::Result;
use super::super::UNSUPPORTED;
use super::Syntax;
use crate::source_description::DescriptionBudget;
use crate::source_description::SourceDescriptionFallback;
use std::str::Chars;
use tree_sitter::Node;

const MAX_TEXT: usize = 16_320;

impl Syntax<'_> {
    pub(super) fn lexical_gaps(&self, node: Node<'_>) -> Result<()> {
        self.budget.check()?;
        if matches!(node.kind(), "string" | "template_string" | "comment") {
            // Contents are lexical data. The finite expression pass separately
            // rejects interpolation and unsupported literal escape forms.
            return Ok(());
        }
        if node.child_count() == 0 {
            return if node.kind() == "program" {
                self.module_whitespace(node.start_byte(), node.end_byte())
            } else {
                Ok(())
            };
        }
        let mut consumed = node.start_byte();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.budget.check()?;
            self.module_whitespace(consumed, child.start_byte())?;
            self.lexical_gaps(child)?;
            consumed = child.end_byte();
        }
        self.module_whitespace(consumed, node.end_byte())
    }

    pub(super) fn module_whitespace(&self, start: usize, end: usize) -> Result<()> {
        for character in self.source.get(start..end).ok_or(UNSUPPORTED)?.chars() {
            self.budget.check()?;
            // ECMAScript WhiteSpace plus LineTerminator code points. Do not
            // substitute Unicode is_whitespace(), which also admits U+0085.
            if !matches!(
                character,
                '\u{0009}'
                    | '\u{000b}'
                    | '\u{000c}'
                    | '\u{0020}'
                    | '\u{00a0}'
                    | '\u{1680}'
                    | '\u{2000}'
                    ..='\u{200a}'
                        | '\u{202f}'
                        | '\u{205f}'
                        | '\u{3000}'
                        | '\u{feff}'
                        | '\n'
                        | '\r'
                        | '\u{2028}'
                        | '\u{2029}'
            ) {
                return Err(UNSUPPORTED);
            }
        }
        Ok(())
    }

    pub(in super::super) fn decimal(&self, node: Node<'_>) -> Result<()> {
        let raw = crate::text(node, self.source);
        if node.kind() != "number"
            || node.child_count() != 0
            || raw.is_empty()
            || !raw.bytes().all(|b| b.is_ascii_digit())
            || (raw.len() > 1 && raw.starts_with('0'))
            || raw.parse::<u64>().map_err(|_| UNSUPPORTED)? > 9_007_199_254_740_991
        {
            return Err(UNSUPPORTED);
        }
        Ok(())
    }

    pub(in super::super) fn literal_text(&self, node: Node<'_>) -> Result<String> {
        if !matches!(node.kind(), "string" | "template_string") {
            return Err(UNSUPPORTED);
        }
        if node.kind() == "template_string" {
            for child in self.parts(node)? {
                if !matches!(child.kind(), "string_fragment" | "escape_sequence")
                    || child.child_count() != 0
                {
                    return Err(UNSUPPORTED);
                }
            }
            self.punctuation(node, &["`"])?;
        }
        cook(crate::text(node, self.source), self.budget)
    }
}

pub(super) fn reserved(name: &str) -> bool {
    matches!(
        name,
        "await"
            | "yield"
            | "implements"
            | "interface"
            | "package"
            | "private"
            | "protected"
            | "public"
            | "static"
            | "let"
            | "enum"
            | "break"
            | "case"
            | "catch"
            | "class"
            | "const"
            | "continue"
            | "debugger"
            | "default"
            | "delete"
            | "do"
            | "else"
            | "export"
            | "extends"
            | "finally"
            | "for"
            | "function"
            | "if"
            | "import"
            | "in"
            | "instanceof"
            | "new"
            | "return"
            | "super"
            | "switch"
            | "this"
            | "throw"
            | "try"
            | "typeof"
            | "var"
            | "void"
            | "while"
            | "with"
            | "null"
            | "true"
            | "false"
    )
}

fn cook(raw: &str, budget: DescriptionBudget) -> Result<String> {
    let delimiter = raw.chars().next().ok_or(UNSUPPORTED)?;
    if !matches!(delimiter, '\'' | '"' | '`') || raw.len() < 2 || !raw.ends_with(delimiter) {
        return Err(UNSUPPORTED);
    }
    let body = raw.get(1..raw.len() - 1).ok_or(UNSUPPORTED)?;
    let mut chars = body.chars();
    let mut output = String::with_capacity(body.len().min(MAX_TEXT));
    while let Some(character) = chars.next() {
        budget.check()?;
        let character = if character == '\\' {
            match chars.next().ok_or(UNSUPPORTED)? {
                '\n' => continue,
                '\r' => {
                    if chars.clone().next() == Some('\n') {
                        let _ = chars.next();
                    }
                    continue;
                }
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                'b' => '\u{8}',
                'f' => '\u{c}',
                'v' => '\u{b}',
                '\\' => '\\',
                '\'' => '\'',
                '"' => '"',
                escaped @ ('`' | '$') if delimiter == '`' => escaped,
                '0' if !chars.clone().next().is_some_and(|c| c.is_ascii_digit()) => '\0',
                'x' => hex(&mut chars, 2)?,
                'u' => hex(&mut chars, 4)?,
                _ => return Err(UNSUPPORTED),
            }
        } else {
            if character == delimiter
                || character == '\r'
                || (character == '\n' && delimiter != '`')
                || (character == '$' && delimiter == '`' && chars.clone().next() == Some('{'))
            {
                return Err(UNSUPPORTED);
            }
            character
        };
        if output
            .len()
            .checked_add(character.len_utf8())
            .ok_or(SourceDescriptionFallback::SourceLimit)?
            > MAX_TEXT
        {
            return Err(SourceDescriptionFallback::SourceLimit);
        }
        output.push(character);
    }
    Ok(output)
}

fn hex(chars: &mut Chars<'_>, width: usize) -> Result<char> {
    let mut value = 0;
    for _ in 0..width {
        value = value * 16
            + chars
                .next()
                .and_then(|c| c.to_digit(16))
                .ok_or(UNSUPPORTED)?;
    }
    char::from_u32(value).ok_or(UNSUPPORTED)
}
