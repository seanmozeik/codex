//! Inert data operations preserve uncertainty without invalidating unrelated paths.
use super::State;
use crate::Effect;
use crate::Transform;
use crate::children;
use crate::value::HandleMode;
use crate::value::Value;
use tree_sitter::Node;

pub(super) fn json_io(
    state: &mut State<'_, '_>,
    node: Node<'_>,
    api: &str,
    args: &[Value],
) -> Option<Value> {
    match api {
        "json.load" => {
            match args.first() {
                Some(Value::Handle { target, .. }) => state.emit(
                    node,
                    Effect::FileRead {
                        target: target.clone(),
                    },
                ),
                Some(Value::Api(api)) if api == "sys.stdin" => {}
                _ => state.gap(node, "JSON input is not a known file or stdin"),
            }
            Some(Value::Data)
        }
        "json.dump" => {
            if let Some(Value::Handle {
                target,
                mode: HandleMode::Write(mode),
            }) = args.get(1)
            {
                state.emit(
                    node,
                    Effect::FileWrite {
                        target: target.clone(),
                        mode: *mode,
                    },
                );
            } else {
                state.gap(node, "JSON output is not a known writable file");
            }
            Some(Value::Unknown)
        }
        _ => None,
    }
}

pub(super) fn call(api: &str, args: &[Value]) -> Option<Value> {
    match api {
        "json.loads" | "JSON.parse" | "json.dumps" | "JSON.stringify" | "sys.stdin.read"
        | "len" | "int" | "float" | "abs" | "round" | "sum" | "sorted" | "list" | "bool"
        | "min" | "max" | "any" | "all" => Some(Value::Data),
        "console.info" | "console.warn" => Some(Value::Unknown),
        "os.path.join" | "path.join" | "pathlib.Path" | "pathlib.PosixPath" => {
            let mut path = String::new();
            for arg in args {
                let Some(part) = arg.text() else {
                    return Some(Value::Unknown);
                };
                if api != "path.join" && part.starts_with('/') {
                    path = part;
                } else if !part.is_empty() || api == "os.path.join" {
                    if !path.is_empty() && !path.ends_with('/') {
                        path.push('/');
                    }
                    path.push_str(&part);
                }
            }
            let path = match api {
                "os.path.join" if args.is_empty() => return Some(Value::Unknown),
                "os.path.join" => path,
                "path.join" => normalize_path(&path),
                _ => pathlib_path(&path),
            };
            Some(if api.starts_with("pathlib.") {
                Value::Path(path)
            } else {
                Value::Text(path)
            })
        }
        _ => None,
    }
}

fn normalize_path(path: &str) -> String {
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|p| *p != "..") => {
                parts.pop();
            }
            ".." if path.starts_with('/') => {}
            _ => parts.push(part),
        }
    }
    let mut result = parts.join("/");
    if result.is_empty() && !path.starts_with('/') {
        result.push('.');
    }
    if path.ends_with('/') && !result.is_empty() {
        result.push('/');
    }
    if path.starts_with('/') {
        format!("/{result}")
    } else {
        result
    }
}

fn pathlib_path(path: &str) -> String {
    // pathlib removes dots and repeated separators, but preserves parent components
    // because resolving them lexically can change the target through a symlink.
    let root = if path.starts_with("//") && !path.starts_with("///") {
        "//"
    } else if path.starts_with('/') {
        "/"
    } else {
        ""
    };
    let parts: Vec<_> = path
        .split('/')
        .filter(|part| !matches!(*part, "" | "."))
        .collect();
    let result = format!("{root}{}", parts.join("/"));
    if result.is_empty() {
        ".".into()
    } else {
        result
    }
}

pub(super) fn method(value: &Value, method: &str) -> Option<Value> {
    if !matches!(
        value,
        Value::Data | Value::Text(_) | Value::Content { .. } | Value::Object(_) | Value::Array(_)
    ) {
        return None;
    }
    let transform = match method {
        "replace" | "replaceAll" => Some(Transform::TextReplacement),
        "slice" | "substring" => Some(Transform::TextSlice),
        "strip" | "trim" | "lower" | "upper" | "toLowerCase" | "toUpperCase" => {
            Some(Transform::TextNormalization)
        }
        "get" | "keys" | "values" | "items" | "count" | "index" | "split" | "splitlines"
        | "startswith" | "endswith" | "startsWith" | "endsWith" | "includes" | "find"
        | "indexOf" => None,
        _ => return None,
    };
    if let (Some(transform), Value::Content { target, transforms }) = (transform, value) {
        let mut transforms = transforms.clone();
        if transforms.len() < 32 {
            transforms.push(transform);
        }
        Some(Value::Content {
            target: target.clone(),
            transforms,
        })
    } else {
        Some(Value::Data)
    }
}

impl<'tree> State<'_, 'tree> {
    pub(super) fn eval_template(&mut self, node: Node<'tree>, depth: usize) -> Value {
        let mut result = String::new();
        let mut complete = true;
        for part in children(node) {
            if part.kind() == "template_substitution" {
                let value = part
                    .named_child(0)
                    .map_or(Value::Unknown, |n| self.eval(n, depth + 1));
                if let Some(value) = value.text() {
                    result.push_str(&value);
                } else {
                    complete = false;
                }
            } else {
                let literal = format!("`{}`", crate::text(part, self.source));
                if let Some(value) =
                    crate::value::string_literal(&literal, crate::Language::TypeScript)
                {
                    result.push_str(&value);
                } else {
                    complete = false;
                }
            }
            if result.len() > 16_384 {
                complete = false;
                break;
            }
        }
        if complete {
            Value::Text(result)
        } else {
            self.gap(node, "Template content is unresolved");
            Value::Unknown
        }
    }

    pub(super) fn eval_format(&mut self, node: Node<'tree>, depth: usize) -> Value {
        for interpolation in children(node)
            .into_iter()
            .filter(|n| n.kind() == "interpolation")
        {
            for part in children(interpolation) {
                self.eval(part, depth + 1);
            }
        }
        Value::Data
    }
}
