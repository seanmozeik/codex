//! Conservative JavaScript string decoding; no Python escape semantics reused.

use super::Deadline;
use super::OrchestrationFallback;
use super::input::MAX_TEXT;
use std::str::Chars;

pub(super) fn decode(raw: &str, deadline: Deadline) -> Result<String, OrchestrationFallback> {
    let unsupported = OrchestrationFallback::UnsupportedSource;
    let quote = raw.chars().next().ok_or(unsupported)?;
    if !matches!(quote, '\'' | '"') || raw.len() < 2 || !raw.ends_with(quote) {
        return Err(unsupported);
    }
    let body = raw.get(1..raw.len() - 1).ok_or(unsupported)?;
    let mut chars = body.chars();
    let mut output = String::with_capacity(body.len().min(MAX_TEXT));
    while let Some(character) = chars.next() {
        deadline.remaining()?;
        let character = if character == '\\' {
            match chars.next().ok_or(unsupported)? {
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
                '0' if !chars.clone().next().is_some_and(|c| c.is_ascii_digit()) => '\0',
                'x' => hex(&mut chars, 2)?,
                'u' => hex(&mut chars, 4)?,
                _ => return Err(unsupported),
            }
        } else {
            if character == quote || matches!(character, '\r' | '\n') {
                return Err(unsupported);
            }
            character
        };
        if output.len() + character.len_utf8() > MAX_TEXT {
            return Err(OrchestrationFallback::ResourceLimit);
        }
        output.push(character);
    }
    Ok(output)
}

fn hex(chars: &mut Chars<'_>, width: usize) -> Result<char, OrchestrationFallback> {
    let unsupported = OrchestrationFallback::UnsupportedSource;
    let mut value = 0;
    for _ in 0..width {
        value = value * 16
            + chars
                .next()
                .and_then(|c| c.to_digit(16))
                .ok_or(unsupported)?;
    }
    // Lone and paired UTF-16 surrogate spellings are conservatively unsupported.
    char::from_u32(value).ok_or(unsupported)
}
