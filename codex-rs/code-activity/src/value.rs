use crate::Language;
use crate::Target;
use crate::Transform;
use crate::WriteMode;
use std::collections::BTreeMap;

pub const MAX_VALUE_BYTES: usize = 16_384;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FunctionId(pub usize);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Data,
    Function(FunctionId),
    Deferred(Box<Self>),
    Boolean(bool),
    Text(String),
    Path(String),
    Api(String),
    Member(Box<Self>, String),
    Content {
        target: Target,
        transforms: Vec<Transform>,
    },
    Handle {
        target: Target,
        mode: HandleMode,
    },
    Array(Vec<Self>),
    Object(BTreeMap<String, Self>),
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HandleMode {
    Read,
    Write(WriteMode),
    Unknown,
}

impl Value {
    pub(crate) fn within_budget(&self) -> bool {
        self.budget_size().is_some()
    }

    pub(crate) fn budget_size(&self) -> Option<usize> {
        fn visit(value: &Value, remaining: &mut usize, depth: usize) -> bool {
            if depth > 64 || *remaining < 64 {
                return false;
            }
            *remaining -= 64;
            let text_len = match value {
                Value::Text(s) | Value::Path(s) | Value::Api(s) => s.len(),
                Value::Content { target, transforms } => {
                    target_size(target) + transforms.len() * std::mem::size_of::<Transform>()
                }
                Value::Handle { target, .. } => target_size(target),
                Value::Deferred(value) => return visit(value, remaining, depth + 1),
                Value::Member(base, name) => {
                    if !visit(base, remaining, depth + 1) {
                        return false;
                    }
                    name.len()
                }
                Value::Array(values) => {
                    return values.iter().all(|v| visit(v, remaining, depth + 1));
                }
                Value::Object(values) => {
                    for (key, value) in values {
                        if key.len() > *remaining {
                            return false;
                        }
                        *remaining -= key.len();
                        if !visit(value, remaining, depth + 1) {
                            return false;
                        }
                    }
                    return true;
                }
                Value::Unknown | Value::Data | Value::Boolean(_) | Value::Function(_) => 0,
            };
            if text_len > *remaining {
                return false;
            }
            *remaining -= text_len;
            true
        }
        fn target_size(target: &Target) -> usize {
            match target {
                Target::Literal { path, cwd } => path.len() + cwd.as_ref().map_or(0, String::len),
                Target::Unresolved => 0,
            }
        }
        let mut remaining = MAX_VALUE_BYTES;
        visit(self, &mut remaining, 0).then_some(MAX_VALUE_BYTES - remaining)
    }
    pub(crate) fn text(&self) -> Option<String> {
        match self {
            Self::Text(s) | Self::Path(s) => Some(s.clone()),
            _ => None,
        }
    }
    pub(crate) fn target(&self, cwd: Option<&str>) -> Target {
        self.text()
            .map_or(Target::Unresolved, |path| Target::Literal {
                path,
                cwd: cwd.map(str::to_owned),
            })
    }
}

/// Decode only literal forms we can preserve exactly. Unsupported escapes stay unknown.
pub fn string_literal(raw: &str, language: Language) -> Option<String> {
    let python = language == Language::Python;
    let mut raw = raw;
    let mut is_raw = false;
    if python {
        let prefix_len = raw.find(['\'', '"'])?;
        let prefix = raw[..prefix_len].to_lowercase();
        if !matches!(prefix.as_str(), "" | "r" | "u") {
            return None;
        }
        is_raw = prefix == "r";
        raw = &raw[prefix_len..];
    }
    let q = raw.chars().next()?;
    if !matches!(q, '\'' | '"' | '`') {
        return None;
    }
    let width = if python && (raw.starts_with("\"\"\"") || raw.starts_with("'''")) {
        3
    } else {
        1
    };
    if raw.len() < width * 2 || !raw.ends_with(&raw[..width]) {
        return None;
    }
    let body = &raw[width..raw.len() - width];
    if is_raw {
        return Some(body.to_owned());
    }
    let mut out = String::new();
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        out.push(match chars.next()? {
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            '\\' => '\\',
            '\'' => '\'',
            '"' => '"',
            '`' => '`',
            '\n' => continue,
            'b' => '\u{8}',
            'f' => '\u{c}',
            'v' => '\u{b}',
            kind @ ('u' | 'x') => {
                let width = if kind == 'x' { 2 } else { 4 };
                let digits: String = chars.by_ref().take(width).collect();
                if digits.len() != width {
                    return None;
                }
                char::from_u32(u32::from_str_radix(&digits, 16).ok()?)?
            }
            _ => return None,
        });
    }
    Some(out)
}
