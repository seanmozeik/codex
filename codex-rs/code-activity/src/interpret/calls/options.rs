//! Operation modes and explicit destructive APIs have one semantic owner.
use crate::Effect;
use crate::Target;
use crate::WriteMode;
use crate::value::Value;
use std::collections::BTreeMap;

pub(super) fn explicit_effect(api: &str, target: Target) -> Option<Effect> {
    match api {
        "os.remove" | "os.unlink" | "os.rmdir" | "shutil.rmtree" | "fs.rm" | "fs.rmSync"
        | "fs.unlink" | "fs.unlinkSync" | "fs.rmdir" | "fs.rmdirSync" | "Deno.remove"
        | "Deno.removeSync" => Some(Effect::FileDelete { target }),
        "os.truncate" | "fs.truncate" | "fs.truncateSync" => Some(Effect::FileTruncate { target }),
        _ => None,
    }
}

pub(super) fn write_mode(api: &str, options: Option<&Value>) -> WriteMode {
    let default = if api.starts_with("fs.append") {
        WriteMode::Append
    } else {
        WriteMode::Replace
    };
    match options {
        None => default,
        Some(Value::Text(_)) if api.starts_with("fs.") => default,
        Some(Value::Object(fields)) if api.starts_with("fs.") => match fields.get("flag") {
            None => default,
            Some(Value::Text(flag)) => match flag.as_str() {
                "w" | "w+" => WriteMode::Replace,
                "wx" | "wx+" | "ax" | "ax+" => WriteMode::ExclusiveCreate,
                "a" | "a+" => WriteMode::Append,
                _ => WriteMode::Unknown,
            },
            _ => WriteMode::Unknown,
        },
        Some(Value::Object(fields)) if api.starts_with("Deno.") => match fields.get("append") {
            None | Some(Value::Boolean(false)) => WriteMode::Replace,
            Some(Value::Boolean(true)) => WriteMode::Append,
            _ => WriteMode::Unknown,
        },
        _ => WriteMode::Unknown,
    }
}

pub(super) fn uncertainty(
    function: &Value,
    args: &[Value],
    keywords: &BTreeMap<String, Value>,
) -> Option<&'static str> {
    let name = match function {
        Value::Api(name) | Value::Member(_, name) => name.as_str(),
        _ => return None,
    };
    for (keyword, value) in keywords {
        let valid = match keyword.as_str() {
            "mode" => matches!(name, "open" | "pathlib.Path.open"),
            "encoding" => {
                matches!(name, "open" | "read_text" | "write_text")
                    && encoding(value, /*python*/ true)
            }
            "indent" => matches!(name, "json.dump" | "json.dumps"),
            _ => false,
        };
        if !valid {
            return Some("Call options may invoke uninspected code");
        }
    }
    if name == "replace" && args.len() == 3 && matches!(args.get(2), Some(Value::Text(_))) {
        return Some("Text replacement count is not an integer");
    }
    if matches!(name, "write_text" | "write_bytes" | "write") && args.len() != 1 {
        return Some("File write options are not inspected");
    }
    if matches!(
        name,
        "json.loads" | "JSON.parse" | "json.dumps" | "JSON.stringify"
    ) && args.len() != 1
    {
        return Some("JSON callbacks or options are not inspected");
    }
    if name.starts_with("fs.") && name.contains("File") {
        let option = if name.contains("read") {
            args.get(1)
        } else {
            args.get(2)
        };
        match option {
            Some(value @ Value::Text(_)) if !encoding(value, /*python*/ false) => {
                return Some("Custom file encoding is not inspected");
            }
            Some(Value::Object(fields)) => {
                if fields
                    .get("encoding")
                    .is_some_and(|v| !encoding(v, /*python*/ false))
                {
                    return Some("Custom file encoding is not inspected");
                }
                if name.contains("read")
                    && fields
                        .get("flag")
                        .is_some_and(|v| !matches!(v, Value::Text(s) if s == "r"))
                {
                    return Some("Read options may mutate the target");
                }
            }
            None | Some(Value::Text(_)) => {}
            _ => return Some("File options are unresolved"),
        }
    }
    None
}

fn encoding(value: &Value, python: bool) -> bool {
    let Value::Text(value) = value else {
        return false;
    };
    let encoding = value.to_ascii_lowercase().replace(['-', '_'], "");
    matches!(encoding.as_str(), "utf8" | "ascii" | "latin1")
        || if python {
            matches!(
                encoding.as_str(),
                "utf16" | "utf16le" | "utf16be" | "utf32" | "utf32le" | "utf32be"
            )
        } else {
            matches!(
                encoding.as_str(),
                "utf16le" | "ucs2" | "hex" | "base64" | "base64url" | "binary"
            )
        }
}

/// Callback-bearing operations must not preserve state they may mutate.
pub(super) fn unsupported_callback(
    function: &Value,
    args: &[Value],
    keywords: &BTreeMap<String, Value>,
) -> bool {
    let callback = args
        .iter()
        .chain(keywords.values())
        .any(|value| may_run_code(value, /*depth*/ 0));
    let unknown_callback = match function {
        Value::Api(api) if matches!(api.as_str(), "JSON.parse" | "JSON.stringify") => {
            matches!(args.get(1), Some(Value::Unknown))
        }
        Value::Api(api) if matches!(api.as_str(), "sorted" | "min" | "max") => {
            matches!(keywords.get("key"), Some(Value::Unknown))
        }
        Value::Api(api) if api.starts_with("json.") => keywords
            .iter()
            .any(|(key, value)| key != "indent" && matches!(value, Value::Unknown)),
        Value::Api(api) if api == "re.sub" => matches!(args.get(1), Some(Value::Unknown)),
        Value::Member(_, method) if matches!(method.as_str(), "replace" | "replaceAll") => {
            matches!(args.get(1), Some(Value::Unknown))
        }
        _ => false,
    };
    (callback || unknown_callback)
        && (matches!(function, Value::Api(api) if matches!(api.as_str(),
        "re.sub" | "JSON.parse" | "JSON.stringify" | "json.load" | "json.loads"
        | "json.dump" | "json.dumps" | "sorted" | "min" | "max") || api.starts_with("fs."))
            || matches!(function, Value::Member(_, method) if matches!(method.as_str(), "replace" | "replaceAll")))
}

fn may_run_code(value: &Value, depth: usize) -> bool {
    if depth >= 64 {
        return true;
    }
    match value {
        Value::Function(_) => true,
        Value::Object(fields) => fields.values().any(|value| may_run_code(value, depth + 1)),
        Value::Array(values) => values.iter().any(|value| may_run_code(value, depth + 1)),
        Value::Member(value, _) | Value::Deferred(value) => may_run_code(value, depth + 1),
        _ => false,
    }
}
