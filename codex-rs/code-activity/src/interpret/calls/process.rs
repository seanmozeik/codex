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
        if let Some(Value::Text(source) | Value::Path(source)) = fields.get("cmd") {
            if !embedded_capacity(state, node) || !embedded_size(state, node, source.len()) {
                return;
            }
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
                source: source.clone(),
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
        if !embedded_capacity(state, node) || !embedded_size(state, node, source.len()) {
            return;
        }
        state.embedded.push(Embedded {
            source: source.to_owned(),
            language,
            cwd: state.cwd.clone(),
            parent: span(node, state.source_id),
        });
    }
}

fn embedded_capacity(state: &mut State<'_, '_>, node: Node<'_>) -> bool {
    if state.report.sources.len() + state.embedded.len() >= 64 {
        state.gap(node, "Embedded source budget exceeded");
        false
    } else {
        true
    }
}

fn embedded_size(state: &mut State<'_, '_>, node: Node<'_>, bytes: usize) -> bool {
    if bytes > crate::MAX_SOURCE_BYTES {
        state.gap(node, "Embedded source byte budget exceeded");
        false
    } else {
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::Analyzer;
    use crate::Language;
    use crate::Request;
    use crate::SourceId;
    use pretty_assertions::assert_eq;

    #[test]
    fn repeated_payload_aliases_bound_the_pending_queue() -> anyhow::Result<()> {
        let payload = "#".to_owned() + &"a".repeat(8_000);
        for calls in [
            "tools.exec_command({cmd: code, login: false});".repeat(65),
            "const cp=require('child_process');".to_owned()
                + &"cp.spawn('python3',['-c',code]);".repeat(65),
        ] {
            let source = format!("const code='{payload}';{calls}");
            let mut analyzer = Analyzer::new()?;
            let mut report = analyzer.analyze(Request::new(
                "queue-bound".into(),
                Language::TypeScript,
                String::new(),
                /*cwd*/ None,
            ));
            let tree = analyzer
                .typescript
                .parse(&source, None)
                .ok_or_else(|| anyhow::anyhow!("cannot parse inert queue fixture"))?;
            let pending = crate::interpret::analyze(
                tree.root_node(),
                &source,
                SourceId(0),
                Language::TypeScript,
                /*cwd*/ None,
                &mut report,
            );
            assert_eq!(pending.len(), 63);
            assert!(pending.iter().all(|entry| entry.source == payload));
            assert!(
                report
                    .unresolved
                    .iter()
                    .any(|gap| gap.reason == "Embedded source budget exceeded")
            );
        }
        Ok(())
    }
}
