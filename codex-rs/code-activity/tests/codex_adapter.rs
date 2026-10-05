//! Real Codex core item boundary fixtures; none of the submitted source executes.

use codex_code_activity::Analyzer;
use codex_code_activity::Basis;
use codex_code_activity::CodexCommandActivity;
use codex_code_activity::CodexSourceActivity;
use codex_code_activity::Coverage;
use codex_code_activity::Effect;
use codex_code_activity::MAX_SOURCE_BYTES;
use codex_code_activity::Report;
use codex_code_activity::Target;
use codex_code_activity::UnsupportedCodexCarrier;
use codex_code_activity::analyze_codex_command;
use codex_protocol::items::CommandExecutionItem;
use codex_protocol::items::CommandExecutionStatus;
use codex_protocol::parse_command::ParsedCommand;
use codex_protocol::protocol::ExecCommandSource;
use codex_shell_command::parse_command::parse_command;
use codex_utils_path_uri::PathUri;
use pretty_assertions::assert_eq;

const EDIT_SCRIPT: &str = "python3 - <<'PY'\nfrom pathlib import Path\np=Path('settings.ts')\np.write_text(p.read_text().replace('old', 'new'))\nPY";

fn command_item(shell: &str, script: &str, cwd: &str) -> anyhow::Result<CommandExecutionItem> {
    let command = [shell, "-lc", script].map(str::to_owned).to_vec();
    Ok(CommandExecutionItem {
        sandbox_type: None,
        model_context: None,
        id: "fixture-command".into(),
        plugin_id: None,
        script_path: None,
        process_id: Some("fixture-process".into()),
        parsed_cmd: parse_command(&command),
        command,
        cwd: PathUri::parse(cwd)?,
        source: ExecCommandSource::Agent,
        interaction_input: None,
        status: CommandExecutionStatus::InProgress,
        aggregated_output: None,
        exit_code: None,
        duration: None,
    })
}

fn report<'a>(activity: &'a CodexCommandActivity<'_>) -> anyhow::Result<&'a Report> {
    match &activity.source_activity {
        CodexSourceActivity::Supported { report, .. } => Ok(report),
        other => anyhow::bail!("expected source report, received {other:?}"),
    }
}

#[test]
fn real_codex_extractor_preserves_shell_and_nested_source_intent() -> anyhow::Result<()> {
    for shell in ["/bin/bash", "/bin/zsh", "/bin/sh"] {
        let item = command_item(shell, EDIT_SCRIPT, "file:///workspace")?;
        let activity = analyze_codex_command(&mut Analyzer::new()?, &item);
        let report = report(&activity)?;
        assert_eq!(report.call_id.as_str(), item.id);
        assert_eq!(report.coverage, Coverage::Partial);
        assert!(matches!(
            &activity.source_activity,
            CodexSourceActivity::Supported { shell: actual, .. } if *actual == shell
        ));
        assert!(report.sources.iter().any(|source| source.parent.is_some()));
        assert!(report.operations.iter().any(|operation| matches!(
            &operation.effect,
            Effect::FileEdit { target: Target::Literal { path, cwd }, .. }
                if path == "settings.ts" && cwd.is_none()
        )));
        assert!(
            report
                .operations
                .iter()
                .all(|op| op.basis == Basis::StaticIntent)
        );
        assert!(report.diffs.is_empty());
    }
    Ok(())
}

#[test]
fn original_item_and_conservative_actions_are_preserved() -> anyhow::Result<()> {
    let item = command_item("/bin/bash", EDIT_SCRIPT, "file:///workspace")?;
    let before = serde_json::to_value(&item)?;
    let activity = analyze_codex_command(&mut Analyzer::new()?, &item);
    assert!(std::ptr::eq(
        std::ptr::from_ref(activity.item),
        std::ptr::from_ref(&item)
    ));
    assert_eq!(serde_json::to_value(activity.item)?, before);
    assert_eq!(activity.item.parsed_cmd.len(), 1);
    assert!(matches!(
        activity.item.parsed_cmd[0],
        ParsedCommand::Unknown { .. }
    ));
    Ok(())
}

#[test]
fn nested_typescript_child_source_remains_static_provenance() -> anyhow::Result<()> {
    let script = r#"node - <<'JS'
const cmd = "python3 -c \"open('nested.txt').read()\"";
await tools.exec_command({cmd});
JS"#;
    let item = command_item("/bin/bash", script, "file:///workspace")?;
    let activity = analyze_codex_command(&mut Analyzer::new()?, &item);
    let report = report(&activity)?;
    assert!(report.sources.len() >= 4);
    assert!(
        report
            .sources
            .iter()
            .skip(1)
            .all(|source| source.parent.is_some())
    );
    assert!(report.operations.iter().any(|operation| matches!(
        &operation.effect,
        Effect::FileRead { target: Target::Literal { path, cwd } }
            if path == "nested.txt" && cwd.is_none()
    )));
    assert!(
        report
            .operations
            .iter()
            .all(|operation| operation.basis == Basis::StaticIntent)
    );
    assert!(report.diffs.is_empty());
    Ok(())
}

#[test]
fn lifecycle_status_never_turns_intent_into_observed_changes() -> anyhow::Result<()> {
    let mut analyzer = Analyzer::new()?;
    let mut item = command_item("/bin/bash", EDIT_SCRIPT, "file:///workspace")?;
    let initial = serde_json::to_value(report(&analyze_codex_command(&mut analyzer, &item))?)?;
    for (status, exit_code) in [
        (CommandExecutionStatus::InProgress, None),
        (CommandExecutionStatus::Completed, Some(0)),
        (CommandExecutionStatus::Failed, Some(1)),
        (CommandExecutionStatus::Declined, None),
    ] {
        item.status = status;
        item.exit_code = exit_code;
        item.aggregated_output = Some("Wrote settings.ts successfully".into());
        let activity = analyze_codex_command(&mut analyzer, &item);
        assert_eq!(serde_json::to_value(report(&activity)?)?, initial);
        assert_eq!(activity.item.status, status);
        assert_eq!(activity.item.exit_code, exit_code);
        assert_eq!(
            activity.item.aggregated_output.as_deref(),
            Some("Wrote settings.ts successfully")
        );
    }
    Ok(())
}

#[test]
fn executor_uris_are_retained_without_host_path_resolution() -> anyhow::Result<()> {
    for cwd in [
        "file:///workspace",
        "file:///C:/foreign%20workspace",
        "file://server/share/workspace",
    ] {
        let item = command_item("/bin/bash", EDIT_SCRIPT, cwd)?;
        let activity = analyze_codex_command(&mut Analyzer::new()?, &item);
        assert_eq!(activity.item.cwd.to_string(), cwd);
        assert!(report(&activity)?.operations.iter().all(|operation| {
            match &operation.effect {
                Effect::FileRead {
                    target: Target::Literal { cwd, .. },
                }
                | Effect::FileEdit {
                    target: Target::Literal { cwd, .. },
                    ..
                } => cwd.is_none(),
                _ => true,
            }
        }));
    }
    Ok(())
}

#[test]
fn powershell_carriers_explicitly_abstain_instead_of_using_bash() -> anyhow::Result<()> {
    let mut item = command_item("/bin/bash", "", "file:///workspace")?;
    item.command = ["pwsh", "-Command", "Get-Content settings.ts"]
        .map(str::to_owned)
        .to_vec();
    item.parsed_cmd = parse_command(&item.command);
    let activity = analyze_codex_command(&mut Analyzer::new()?, &item);
    assert!(matches!(
        activity.source_activity,
        CodexSourceActivity::Unsupported {
            reason: UnsupportedCodexCarrier::UnsupportedShell
        }
    ));
    Ok(())
}

#[test]
fn direct_python_argv_and_missing_source_are_explicitly_unsupported() -> anyhow::Result<()> {
    for command in [
        vec!["python3", "-c", "open('settings.ts').read()"],
        vec!["/bin/bash", "-lc"],
        vec![],
    ] {
        let mut item = command_item("/bin/bash", "", "file:///workspace")?;
        item.command = command.into_iter().map(str::to_owned).collect();
        let activity = analyze_codex_command(&mut Analyzer::new()?, &item);
        assert!(matches!(
            activity.source_activity,
            CodexSourceActivity::Unsupported {
                reason: UnsupportedCodexCarrier::UnrecognizedArgv
            }
        ));
    }
    Ok(())
}

#[test]
fn mixed_known_and_unknown_source_keeps_gaps_and_legacy_unknown() -> anyhow::Result<()> {
    let script =
        "python3 - <<'PY'\nfrom pathlib import Path\nPath('a').read_text()\ncustom_mutation()\nPY";
    let item = command_item("/bin/bash", script, "file:///workspace")?;
    let activity = analyze_codex_command(&mut Analyzer::new()?, &item);
    let report = report(&activity)?;
    assert_eq!(report.coverage, Coverage::Partial);
    assert!(
        report
            .operations
            .iter()
            .any(|operation| matches!(operation.effect, Effect::FileRead { .. }))
    );
    assert!(!report.unresolved.is_empty());
    assert!(matches!(
        activity.item.parsed_cmd.as_slice(),
        [ParsedCommand::Unknown { .. }]
    ));
    Ok(())
}

#[test]
fn source_mentions_in_comments_and_strings_do_not_invent_file_effects() -> anyhow::Result<()> {
    let script =
        "python3 - <<'PY'\n# Path('a').write_text('x')\nprint(\"open('secret').read()\")\nPY";
    let item = command_item("/bin/bash", script, "file:///workspace")?;
    let activity = analyze_codex_command(&mut Analyzer::new()?, &item);
    assert!(
        report(&activity)?
            .operations
            .iter()
            .all(|operation| { matches!(operation.effect, Effect::ProcessRun { .. }) })
    );
    Ok(())
}

#[test]
fn invalid_final_shell_source_has_no_predicted_operations() -> anyhow::Result<()> {
    let script = "cat settings.ts; echo 'unfinished";
    let item = command_item("/bin/bash", script, "file:///workspace")?;
    let activity = analyze_codex_command(&mut Analyzer::new()?, &item);
    let report = report(&activity)?;
    assert_eq!(report.coverage, Coverage::Opaque);
    assert!(report.operations.is_empty());
    assert!(!report.unresolved.is_empty());
    Ok(())
}

#[test]
fn oversized_source_abstains_before_copying_or_parsing() -> anyhow::Result<()> {
    let script = "x".repeat(MAX_SOURCE_BYTES + 1);
    let item = command_item("/bin/bash", &script, "file:///workspace")?;
    let activity = analyze_codex_command(&mut Analyzer::new()?, &item);
    assert!(matches!(
        activity.source_activity,
        CodexSourceActivity::Unsupported {
            reason: UnsupportedCodexCarrier::SourceTooLarge
        }
    ));
    Ok(())
}

#[test]
fn serialized_prototype_keeps_independent_status_and_static_basis() -> anyhow::Result<()> {
    let item = command_item("/bin/bash", EDIT_SCRIPT, "file:///workspace")?;
    let activity = analyze_codex_command(&mut Analyzer::new()?, &item);
    let value = serde_json::to_value(&activity)?;
    assert_eq!(value["item"]["status"], "in_progress");
    assert_eq!(value["item"]["cwd"], "file:///workspace");
    assert_eq!(value["sourceActivity"]["type"], "supported");
    assert_eq!(
        value["sourceActivity"]["report"]["diffs"],
        serde_json::json!([])
    );
    assert!(
        report(&activity)?
            .operations
            .iter()
            .all(|op| op.basis == Basis::StaticIntent)
    );
    Ok(())
}
