//! Finite JavaScript string cooking with explicit literal-template validation.

use super::DescriptionBudget;
use super::MAX_TEXT;
use super::Result;
use super::SourceDescriptionFallback;
use super::UNSUPPORTED;
use std::str::Chars;
use tree_sitter::Node;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Form {
    SingleQuoted,
    DoubleQuoted,
    Template,
}

impl Form {
    fn delimiter(self) -> char {
        match self {
            Self::SingleQuoted => '\'',
            Self::DoubleQuoted => '"',
            Self::Template => '`',
        }
    }
}

pub(super) fn quoted(raw: &str, budget: DescriptionBudget) -> Result<String> {
    let form = match raw.chars().next() {
        Some('\'') => Form::SingleQuoted,
        Some('"') => Form::DoubleQuoted,
        _ => return Err(UNSUPPORTED),
    };
    cook(raw, form, budget)
}

pub(super) fn template(node: Node<'_>, source: &str, budget: DescriptionBudget) -> Result<String> {
    budget.check()?;
    if node.kind() != "template_string" || node.has_error() || node.is_missing() {
        return Err(UNSUPPORTED);
    }
    let mut cursor = node.walk();
    let mut children = node.children(&mut cursor);
    if children
        .next()
        .is_none_or(|child| child.kind() != "`" || child.is_named())
    {
        return Err(UNSUPPORTED);
    }
    let mut closed = false;
    for child in children {
        budget.check()?;
        if closed || child.is_missing() {
            return Err(UNSUPPORTED);
        }
        match child.kind() {
            "`" if !child.is_named() => closed = true,
            "string_fragment" | "escape_sequence"
                if child.is_named() && child.child_count() == 0 => {}
            _ => return Err(UNSUPPORTED),
        }
    }
    if !closed {
        return Err(UNSUPPORTED);
    }
    cook(crate::text(node, source), Form::Template, budget)
}

fn cook(raw: &str, form: Form, budget: DescriptionBudget) -> Result<String> {
    let delimiter = form.delimiter();
    if raw.len() < 2 || !raw.starts_with(delimiter) || !raw.ends_with(delimiter) {
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
                escaped @ ('`' | '$') if form == Form::Template => escaped,
                '0' if !chars.clone().next().is_some_and(|c| c.is_ascii_digit()) => '\0',
                'x' => hex(&mut chars, 2)?,
                'u' => hex(&mut chars, 4)?,
                _ => return Err(UNSUPPORTED),
            }
        } else {
            if character == delimiter
                || character == '\r'
                || (character == '\n' && form != Form::Template)
                || (character == '$' && form == Form::Template && chars.clone().next() == Some('{'))
            {
                return Err(UNSUPPORTED);
            }
            character
        };
        if output.len() + character.len_utf8() > MAX_TEXT {
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

#[cfg(test)]
mod tests {
    use super::DescriptionBudget;
    use super::Form;
    use super::Result;
    use super::SourceDescriptionFallback;
    use super::cook;
    use super::quoted;
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    fn budget() -> DescriptionBudget {
        DescriptionBudget::new(Duration::from_secs(1))
    }

    #[test]
    fn literal_template_cooking_preserves_lf_and_removes_line_continuations() -> Result<()> {
        for (raw, expected) in [
            ("`alpha\nbeta`", "alpha\nbeta"),
            ("`alpha\\\nbeta`", "alphabeta"),
            ("`alpha\\\r\nbeta`", "alphabeta"),
            (r#"`cat 'a\`b' '\${name}'`"#, "cat 'a`b' '${name}'"),
            (r#"`'\x61\u0062'`"#, "'ab'"),
            (r#"`\u0024{name}`"#, "${name}"),
            ("`\"'`", "\"'"),
        ] {
            assert_eq!(cook(raw, Form::Template, budget())?, expected, "{raw}");
        }
        Ok(())
    }

    #[test]
    fn unsupported_template_escapes_and_raw_cr_never_get_guessed() {
        for raw in [
            "`a\rb`",
            "`a\r\nb`",
            "`${name}`",
            r"`\xZZ`",
            r"`\8`",
            r"`\01`",
            r"`\q`",
            r"`\uD800`",
            r"`\u{61}`",
        ] {
            assert_eq!(
                cook(raw, Form::Template, budget()),
                Err(SourceDescriptionFallback::UnsupportedSource),
                "{raw}"
            );
        }
    }

    #[test]
    fn common_decoder_keeps_ordinary_quoted_string_rules() -> Result<()> {
        assert_eq!(quoted(r#""a\nb\x63\u0064""#, budget())?, "a\nbcd");
        for raw in ["\"raw\nline\"", r#""\`""#, r#""\$""#, "`template`"] {
            assert_eq!(
                quoted(raw, budget()),
                Err(SourceDescriptionFallback::UnsupportedSource)
            );
        }
        Ok(())
    }

    #[test]
    fn template_cooking_keeps_decoded_byte_and_elapsed_limits() {
        let raw = format!("`{}`", "é".repeat(8_161));
        assert_eq!(
            cook(&raw, Form::Template, budget()),
            Err(SourceDescriptionFallback::SourceLimit)
        );
        assert_eq!(
            cook(
                "`x`",
                Form::Template,
                DescriptionBudget::new(Duration::ZERO)
            ),
            Err(SourceDescriptionFallback::TimeBudget)
        );
    }
}
