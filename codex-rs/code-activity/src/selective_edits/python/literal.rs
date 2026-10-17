//! Closed P0 Python token decoder, reused without executing Python.
//!
//! The lexical rules are copied from source_description/python_origin/literal.rs;
//! only the error vocabulary is adapted to this producer's existing fallback.

use crate::SourceDescriptionFallback as Fallback;

const MAX_TEXT_BYTES: usize = 16_384;

pub(super) fn identifier(raw: &str) -> bool {
    raw != "__debug__"
        && !matches!(
            raw,
            "False"
                | "None"
                | "True"
                | "and"
                | "as"
                | "assert"
                | "async"
                | "await"
                | "break"
                | "class"
                | "continue"
                | "def"
                | "del"
                | "elif"
                | "else"
                | "except"
                | "finally"
                | "for"
                | "from"
                | "global"
                | "if"
                | "import"
                | "in"
                | "is"
                | "lambda"
                | "nonlocal"
                | "not"
                | "or"
                | "pass"
                | "raise"
                | "return"
                | "try"
                | "while"
                | "with"
                | "yield"
        )
        && raw
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && raw
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

pub(super) fn integer(raw: &str) -> Result<i64, Fallback> {
    if raw.is_empty() || !raw.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(Fallback::UnsupportedSource);
    }
    if raw.starts_with('0') && raw.bytes().any(|byte| byte != b'0') {
        return Err(Fallback::UnsupportedSource);
    }
    raw.parse().map_err(|_| Fallback::UnsupportedSource)
}

pub(super) fn decode(raw: &str) -> Result<String, Fallback> {
    if raw.len() > MAX_TEXT_BYTES + 8 {
        return Err(Fallback::SourceLimit);
    }
    if raw.contains(['\r', '\0']) {
        return Err(Fallback::UnsupportedSource);
    }
    let (raw_mode, quoted) = match raw.as_bytes().first() {
        Some(b'r' | b'R') => (true, &raw[1..]),
        Some(b'u' | b'U') => (false, &raw[1..]),
        _ => (false, raw),
    };
    let quote = match quoted.as_bytes().first() {
        Some(b'\'') => '\'',
        Some(b'"') => '"',
        _ => return Err(Fallback::UnsupportedSource),
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
        return Err(Fallback::UnsupportedSource);
    }
    let body = &quoted[width..quoted.len() - width];
    let mut output = String::with_capacity(body.len());
    let mut characters = body.chars();
    let mut quotes = 0;
    while let Some(character) = characters.next() {
        if character == quote {
            quotes += 1;
            if quotes >= width {
                return Err(Fallback::UnsupportedSource);
            }
        } else {
            quotes = 0;
        }
        if character == '\n' && width == 1 {
            return Err(Fallback::UnsupportedSource);
        }
        if character != '\\' {
            output.push(character);
            continue;
        }
        quotes = 0;
        let escaped = characters.next().ok_or(Fallback::UnsupportedSource)?;
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
            _ => return Err(Fallback::UnsupportedSource),
        }
    }
    if output.len() > MAX_TEXT_BYTES {
        return Err(Fallback::SourceLimit);
    }
    Ok(output)
}

fn hex(characters: &mut impl Iterator<Item = char>, digits: usize) -> Result<char, Fallback> {
    let mut value = 0u32;
    for _ in 0..digits {
        let digit = characters
            .next()
            .and_then(|character| character.to_digit(16))
            .ok_or(Fallback::UnsupportedSource)?;
        value = value
            .checked_mul(16)
            .and_then(|value| value.checked_add(digit))
            .ok_or(Fallback::UnsupportedSource)?;
    }
    char::from_u32(value).ok_or(Fallback::UnsupportedSource)
}
