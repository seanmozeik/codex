//! Recognised standard-library calls and content transformations.
mod filesystem;
mod options;
mod process;
use super::State;
use super::imports;
use crate::Effect;
use crate::Target;
use crate::Transform;
use crate::WriteMode;
use crate::value::HandleMode;
use crate::value::Value;
use std::collections::BTreeMap;
use tree_sitter::Node;

pub(super) fn evaluate(
    state: &mut State<'_, '_>,
    node: Node<'_>,
    function: Value,
    args: &[Value],
    keywords: &BTreeMap<String, Value>,
) -> Value {
    if options::unsupported_callback(&function, args, keywords) {
        state.gap(
            node,
            "Replacement or serialization callback is not inspected",
        );
        state.bindings.clear();
        state.cwd = None;
        return Value::Unknown;
    }
    if let Some(reason) = options::uncertainty(&function, args, keywords) {
        state.gap(node, reason);
    }
    let first = args.first().cloned().unwrap_or(Value::Unknown);
    match function {
        Value::Api(api) => evaluate_api(state, node, &api, first, args, keywords),
        Value::Member(receiver, method) => {
            evaluate_member(state, node, *receiver, &method, args, keywords)
        }
        _ => {
            state.gap(node, "Unknown callable; bindings invalidated");
            state.bindings.clear();
            state.cwd = None;
            Value::Unknown
        }
    }
}

fn evaluate_member(
    state: &mut State<'_, '_>,
    node: Node<'_>,
    receiver: Value,
    method: &str,
    args: &[Value],
    keywords: &BTreeMap<String, Value>,
) -> Value {
    if let Some(value) = super::data::method(&receiver, method) {
        return value;
    }
    match (receiver, method) {
        (Value::Path(path), "unlink" | "rmdir") => {
            state.emit(
                node,
                Effect::FileDelete {
                    target: Value::Path(path).target(state.cwd.as_deref()),
                },
            );
            Value::Unknown
        }
        (Value::Handle { target, .. }, "truncate") => {
            state.emit(node, Effect::FileTruncate { target });
            Value::Unknown
        }
        (Value::Handle { .. }, "close") => Value::Unknown,
        (Value::Path(path), "read_text" | "read_bytes") => {
            read(state, node, Value::Path(path).target(state.cwd.as_deref()))
        }
        (Value::Path(path), "write_text" | "write_bytes") => {
            write(
                state,
                node,
                Value::Path(path).target(state.cwd.as_deref()),
                args.first(),
                WriteMode::Replace,
            );
            Value::Unknown
        }
        (Value::Path(path), "open") => open(
            state,
            node,
            &Value::Path(path),
            args.first().or_else(|| keywords.get("mode")),
        ),
        (Value::Path(path), "glob" | "rglob" | "iterdir") => {
            state.emit(
                node,
                Effect::FileList {
                    target: Value::Path(path).target(state.cwd.as_deref()),
                },
            );
            Value::Unknown
        }
        (
            Value::Handle {
                target,
                mode: HandleMode::Read,
            },
            "read" | "readlines" | "text" | "json",
        ) => {
            let value = read(state, node, target);
            if state.language == crate::Language::TypeScript && matches!(method, "text" | "json") {
                Value::Deferred(Box::new(value))
            } else {
                value
            }
        }
        (
            Value::Handle {
                target,
                mode: HandleMode::Write(mode),
            },
            "write",
        ) => {
            write(state, node, target, args.first(), mode);
            Value::Unknown
        }
        _ => {
            state.gap(
                node,
                "Receiver or method cannot be resolved; bindings invalidated",
            );
            state.bindings.clear();
            state.cwd = None;
            Value::Unknown
        }
    }
}

fn read(state: &mut State<'_, '_>, node: Node<'_>, target: Target) -> Value {
    state.emit(
        node,
        Effect::FileRead {
            target: target.clone(),
        },
    );
    Value::Content {
        target,
        transforms: vec![],
    }
}
fn write(
    state: &mut State<'_, '_>,
    node: Node<'_>,
    target: Target,
    content: Option<&Value>,
    mode: WriteMode,
) {
    if matches!(mode, WriteMode::Replace)
        && let Some(Value::Content {
            target: origin,
            transforms,
        }) = content
        && *origin == target
        && !matches!(target, Target::Unresolved)
        && !transforms.is_empty()
    {
        state.emit(
            node,
            Effect::FileEdit {
                target,
                transforms: transforms.clone(),
            },
        );
        return;
    }
    state.emit(node, Effect::FileWrite { target, mode });
}
fn open(state: &mut State<'_, '_>, node: Node<'_>, path: &Value, mode: Option<&Value>) -> Value {
    let target = path.target(state.cwd.as_deref());
    let mode = match mode {
        None => HandleMode::Read,
        Some(Value::Text(s)) => match s.as_str() {
            "r" | "rb" | "rt" => HandleMode::Read,
            "w" | "wb" | "wt" => HandleMode::Write(WriteMode::Replace),
            "a" | "ab" | "at" => HandleMode::Write(WriteMode::Append),
            "x" | "xb" | "xt" => HandleMode::Write(WriteMode::ExclusiveCreate),
            _ => HandleMode::Unknown,
        },
        _ => HandleMode::Unknown,
    };
    if let HandleMode::Write(write_mode) = &mode {
        state.emit(
            node,
            Effect::FileOpen {
                target: target.clone(),
                mode: *write_mode,
            },
        );
    } else if matches!(mode, HandleMode::Unknown) {
        state.emit(
            node,
            Effect::FileOpen {
                target: target.clone(),
                mode: WriteMode::Unknown,
            },
        );
        state.gap(
            node,
            "File open mode is unresolved; it may mutate the target",
        );
    }
    Value::Handle { target, mode }
}
fn transform(value: Value, method: Transform) -> Value {
    match value {
        Value::Content {
            target,
            mut transforms,
        } => {
            if transforms.len() < 32 {
                transforms.push(method);
            }
            Value::Content { target, transforms }
        }
        _ => Value::Unknown,
    }
}

fn evaluate_api(
    state: &mut State<'_, '_>,
    node: Node<'_>,
    api: &str,
    first: Value,
    args: &[Value],
    keywords: &BTreeMap<String, Value>,
) -> Value {
    if let Some(value) = super::data::call(api, args) {
        return value;
    }
    if let Some(value) = super::data::json_io(state, node, api, args) {
        return value;
    }
    if let Some(value) = filesystem::evaluate(state, node, api, &first, args) {
        return value;
    }
    if let Some(effect) = options::explicit_effect(api, first.target(state.cwd.as_deref())) {
        state.emit(node, effect);
        return Value::Data;
    }
    match api {
        "os.chdir" | "process.chdir" => {
            state.gap(node, "Working directory mutation is not resolved");
            state.cwd = None;
            Value::Unknown
        }
        "open" => open(
            state,
            node,
            &first,
            args.get(1).or_else(|| keywords.get("mode")),
        ),
        "str" => match first {
            Value::Path(s) => Value::Text(s),
            v => v,
        },
        "print" | "text" | "console.log" | "console.error" => Value::Unknown,
        "os.listdir" | "os.walk" => {
            state.emit(
                node,
                Effect::FileList {
                    target: first.target(state.cwd.as_deref()),
                },
            );
            Value::Unknown
        }
        "require" => imports::load(state, node, &first),
        "import" => Value::Deferred(Box::new(imports::load(state, node, &first))),
        "Bun.file" => Value::Handle {
            target: first.target(state.cwd.as_deref()),
            mode: HandleMode::Read,
        },
        "re.sub" => transform(
            args.get(2).cloned().unwrap_or(Value::Unknown),
            Transform::RegexSubstitution,
        ),
        "subprocess.run"
        | "subprocess.call"
        | "subprocess.check_output"
        | "subprocess.check_call"
        | "subprocess.Popen"
        | "child_process.exec"
        | "child_process.execSync"
        | "child_process.spawn"
        | "child_process.spawnSync"
        | "child_process.execFile"
        | "child_process.execFileSync" => {
            process::evaluate(state, node, api, &first, args, keywords);
            Value::Unknown
        }
        "tools.exec_command" => {
            process::tool_command(state, node, first);
            // The host tool returns a Promise. Its child process does not
            // replace the caller's JavaScript lexical bindings on await.
            Value::Deferred(Box::new(Value::Data))
        }
        _ => {
            state.gap(node, "Unsupported API call; effects are unknown");
            state.bindings.clear();
            state.cwd = None;
            Value::Unknown
        }
    }
}
