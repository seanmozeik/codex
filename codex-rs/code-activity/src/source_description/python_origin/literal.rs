//! Shared conservative lexical values; never runtime Python evaluation.

use crate::source_description::python::origin::shared::Fallback;

/// Decoded UTF-8 limit for one literal.
pub const MAX_TEXT_BYTES: usize = 16_384;

/// Admit unnormalized ASCII names, excluding Python's immutable debug binding.
pub fn identifier(raw: &str) -> bool {
    raw != "__debug__"
        && raw
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && raw
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// Decode an unsigned decimal token; unary signs are separate IR nodes.
pub fn integer(raw: &str) -> Result<i64, Fallback> {
    if raw.is_empty() || !raw.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(Fallback::Unsupported);
    }
    if raw.starts_with('0') && raw.bytes().any(|byte| byte != b'0') {
        return Err(Fallback::Syntax);
    }
    raw.parse().map_err(|_| Fallback::Unsupported)
}

/// Decode one supported Python string token, rejecting ambiguous escape forms.
pub fn decode(raw: &str) -> Result<String, Fallback> {
    if raw.len() > MAX_TEXT_BYTES + 8 {
        return Err(Fallback::Limit);
    }
    if raw.contains(['\r', '\0']) {
        return Err(Fallback::Unsupported);
    }
    let (raw_mode, quoted) = match raw.as_bytes().first() {
        Some(b'r' | b'R') => (true, &raw[1..]),
        Some(b'u' | b'U') => (false, &raw[1..]),
        _ => (false, raw),
    };
    let quote = match quoted.as_bytes().first() {
        Some(b'\'') => '\'',
        Some(b'"') => '"',
        _ => return Err(Fallback::Unsupported),
    };
    let width = if quoted
        .as_bytes()
        .get(..3)
        .is_some_and(|bytes| bytes.iter().all(|byte| *byte == quote as u8))
    {
        3
    } else {
        1
    };
    if quoted.len() < width * 2
        || !quoted.as_bytes()[quoted.len() - width..]
            .iter()
            .all(|byte| *byte == quote as u8)
    {
        return Err(Fallback::Unsupported);
    }
    let body = &quoted[width..quoted.len() - width];
    let mut output = String::with_capacity(body.len());
    let mut characters = body.chars();
    let mut quotes = 0;
    while let Some(character) = characters.next() {
        if character == quote {
            quotes += 1;
            if quotes >= width {
                return Err(Fallback::Unsupported);
            }
        } else {
            quotes = 0;
        }
        if character == '\n' && width == 1 {
            return Err(Fallback::Unsupported);
        }
        if character != '\\' {
            output.push(character);
            continue;
        }
        quotes = 0;
        let escaped = characters.next().ok_or(Fallback::Unsupported)?;
        if raw_mode {
            output.push('\\');
            output.push(escaped);
            continue;
        }
        match escaped {
            '\n' => {}
            '\\' | '\'' | '"' => output.push(escaped),
            'a' => output.push('\u{7}'),
            'b' => output.push('\u{8}'),
            'f' => output.push('\u{c}'),
            'n' => output.push('\n'),
            'r' => output.push('\r'),
            't' => output.push('\t'),
            'v' => output.push('\u{b}'),
            'x' => output.push(hex(&mut characters, 2)?),
            'u' => output.push(hex(&mut characters, 4)?),
            'U' => output.push(hex(&mut characters, 8)?),
            _ => return Err(Fallback::Unsupported),
        }
    }
    if output.len() > MAX_TEXT_BYTES {
        return Err(Fallback::Limit);
    }
    Ok(output)
}

fn hex(characters: &mut impl Iterator<Item = char>, digits: usize) -> Result<char, Fallback> {
    let mut value = 0u32;
    for _ in 0..digits {
        let digit = characters
            .next()
            .and_then(|character| character.to_digit(16))
            .ok_or(Fallback::Unsupported)?;
        value = value
            .checked_mul(16)
            .and_then(|value| value.checked_add(digit))
            .ok_or(Fallback::Unsupported)?;
    }
    char::from_u32(value).ok_or(Fallback::Unsupported)
}
