//! Filesystem API return semantics distinguish promises, callbacks and content.
use super::options;
use crate::Effect;
use crate::interpret::State;
use crate::value::Value;
use std::borrow::Cow;
use tree_sitter::Node;

pub(super) fn evaluate(
    state: &mut State<'_, '_>,
    node: Node<'_>,
    api: &str,
    first: &Value,
    args: &[Value],
) -> Option<Value> {
    let (api, deferred) = if let Some(method) = api.strip_prefix("fs.promises.") {
        if !matches!(
            method,
            "readFile"
                | "writeFile"
                | "appendFile"
                | "readdir"
                | "rm"
                | "unlink"
                | "rmdir"
                | "truncate"
        ) {
            state.gap(node, "Unsupported Promise filesystem API");
            return Some(Value::Unknown);
        }
        (Cow::Owned(format!("fs.{method}")), true)
    } else {
        (
            Cow::Borrowed(api),
            matches!(
                api,
                "Deno.readTextFile" | "Deno.writeTextFile" | "Bun.write"
            ),
        )
    };
    let value = match api.as_ref() {
        "fs.readFile" | "fs.readFileSync" | "Deno.readTextFile" | "Deno.readTextFileSync" => {
            read(state, node, &api, first, args, deferred)
        }
        "fs.writeFile"
        | "fs.writeFileSync"
        | "fs.appendFile"
        | "fs.appendFileSync"
        | "Bun.write"
        | "Deno.writeTextFile"
        | "Deno.writeTextFileSync" => {
            super::write(
                state,
                node,
                first.target(state.cwd.as_deref()),
                args.get(1),
                options::write_mode(&api, args.get(2)),
            );
            Value::Data
        }
        "fs.readdir" | "fs.readdirSync" => {
            state.emit(
                node,
                Effect::FileList {
                    target: first.target(state.cwd.as_deref()),
                },
            );
            Value::Unknown
        }
        name if name.starts_with("fs.") || name.starts_with("Deno.") => {
            let effect = options::explicit_effect(name, first.target(state.cwd.as_deref()))?;
            state.emit(node, effect);
            Value::Data
        }
        _ => return None,
    };
    Some(if deferred {
        Value::Deferred(Box::new(value))
    } else {
        value
    })
}

fn read(
    state: &mut State<'_, '_>,
    node: Node<'_>,
    api: &str,
    first: &Value,
    args: &[Value],
    deferred: bool,
) -> Value {
    if api.starts_with("fs.")
        && let Some(Value::Object(fields)) = args.get(1)
        && fields
            .get("flag")
            .is_some_and(|value| !matches!(value, Value::Text(flag) if flag == "r"))
    {
        state.emit(
            node,
            Effect::FileOpen {
                target: first.target(state.cwd.as_deref()),
                mode: options::write_mode(api, args.get(1)),
            },
        );
    }
    let content = super::read(state, node, first.target(state.cwd.as_deref()));
    if api == "fs.readFile" && !deferred {
        state.gap(
            node,
            "Callback file reads return no immediate content; callback is not inspected",
        );
        Value::Unknown
    } else if api.starts_with("fs.") && !text_encoding(args.get(1)) {
        // Without an encoding Node returns a Buffer, not a string supporting
        // the text transformations used to establish same-file edits.
        Value::Data
    } else {
        content
    }
}

fn text_encoding(option: Option<&Value>) -> bool {
    match option {
        Some(Value::Text(_)) => true,
        Some(Value::Object(fields)) => matches!(fields.get("encoding"), Some(Value::Text(_))),
        _ => false,
    }
}
