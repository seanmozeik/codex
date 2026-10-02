//! Child-process intent; child effects are not executed or inferred.
use super::State;
use super::Value;
use crate::Effect;
use crate::Language;
use crate::interpret::Embedded;
use crate::span;
use std::collections::BTreeMap;
use tree_sitter::Node;

pub(super) fn tool_command(state: &mut State<'_, '_>, node: Node<'_>, argument: Value) {
    if let Value::Object(fields) = argument {
        let shell_supported = match fields.get("shell") {
            None => true,
            Some(Value::Text(shell)) => matches!(
                shell.rsplit('/').next(),
                Some("sh" | "bash" | "zsh" | "dash")
            ),
            _ => false,
        };
        let login_supported = matches!(fields.get("login"), None | Some(Value::Boolean(false)));
        if !shell_supported || !login_supported {
            state.gap(node, "Tool execution context options are not inspected");
            return;
        }
        if let Some(source) = fields.get("cmd").and_then(Value::text) {
            let cwd = match fields.get("workdir") {
                None => state.cwd.clone(),
                Some(Value::Text(path)) if path.starts_with('/') => Some(path.clone()),
                Some(Value::Text(path)) => state.cwd.as_ref().map(|base| format!("{base}/{path}")),
                Some(_) => None,
            };
            if cwd.is_none() {
                state.gap(node, "Tool working directory is unresolved");
            }
            state.embedded.push(Embedded {
                source,
                language: Language::Shell,
                cwd,
                parent: span(node, state.source_id),
            });
        } else {
            state.gap(node, "Dynamic shell command is unresolved");
        }
    } else {
        state.gap(node, "Tool argument is unresolved");
    }
}

pub(super) fn evaluate(
    state: &mut State<'_, '_>,
    node: Node<'_>,
    api: &str,
    first: &Value,
    arguments: &[Value],
    keywords: &BTreeMap<String, Value>,
) {
    match api {
        "subprocess.run"
        | "subprocess.call"
        | "subprocess.check_output"
        | "subprocess.check_call"
        | "subprocess.Popen" => {
            let argv = match first {
                Value::Array(v) => v.iter().map(Value::text).collect(),
                _ => vec![first.text()],
            };
            if keywords.is_empty() && arguments.len() == 1 {
                inspect_child(state, node, &argv);
            }
            state.emit(node, Effect::ProcessRun { argv, shell: None });
            state.gap(
                node,
                "Child process effects and shell option are not evaluated",
            );
        }
        "child_process.exec" | "child_process.execSync" => {
            state.emit(
                node,
                Effect::ProcessRun {
                    argv: vec![],
                    shell: first.text(),
                },
            );
            state.gap(node, "Child process effects are not evaluated");
        }
        "child_process.spawn"
        | "child_process.spawnSync"
        | "child_process.execFile"
        | "child_process.execFileSync" => {
            let mut argv = vec![first.text()];
            if let Some(Value::Array(values)) = arguments.get(1) {
                argv.extend(values.iter().map(Value::text));
            }
            if arguments.len() <= 2 {
                inspect_child(state, node, &argv);
            }
            state.emit(node, Effect::ProcessRun { argv, shell: None });
            state.gap(node, "Child process effects are not evaluated");
        }
        _ => {}
    }
}

fn inspect_child(state: &mut State<'_, '_>, node: Node<'_>, argv: &[Option<String>]) {
    if let Some((language, source)) = crate::shell::interpreter_source(argv) {
        state.embedded.push(Embedded {
            source,
            language,
            cwd: state.cwd.clone(),
            parent: span(node, state.source_id),
        });
    }
}
