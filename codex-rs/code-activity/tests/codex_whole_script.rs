//! Opt-in actual command-item bridge; submitted source is never executed.

use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::Basis;
use codex_code_activity::CodexWholeScriptActivity;
use codex_code_activity::CodexWholeScriptSource;
use codex_code_activity::Effect;
use codex_code_activity::EnvironmentAssumptions;
use codex_code_activity::FallbackReason;
use codex_code_activity::MAX_SOURCE_BYTES;
use codex_code_activity::PresentationDisposition;
use codex_code_activity::Target;
use codex_code_activity::UnsupportedCodexWholeScript;
use codex_code_activity::WholeScriptActivity;
use codex_code_activity::analyze_complete_codex_command;
use codex_protocol::items::CommandExecutionItem;
use codex_protocol::items::CommandExecutionStatus;
use codex_protocol::protocol::ExecCommandSource;
use codex_shell_command::parse_command::parse_command;
use codex_utils_path_uri::PathUri;
use pretty_assertions::assert_eq;
use std::time::Duration;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn item(command: &[&str]) -> TestResult<CommandExecutionItem> {
    let command: Vec<String> = command.iter().map(|arg| (*arg).to_owned()).collect();
    Ok(CommandExecutionItem {
        sandbox_type: None,
        model_context: None,
        id: "strict-command".into(),
        plugin_id: Some("fixture-plugin".into()),
        script_path: Some("scripts/fixture.sh".into()),
        process_id: Some("fixture-process".into()),
        parsed_cmd: parse_command(&command),
        command,
        cwd: PathUri::parse("file:///workspace")?,
        source: ExecCommandSource::Agent,
        interaction_input: None,
        status: CommandExecutionStatus::InProgress,
        aggregated_output: None,
        exit_code: None,
        duration: None,
    })
}

fn limits() -> AnalysisLimits {
    AnalysisLimits::new(MAX_SOURCE_BYTES, Duration::from_secs(5))
}

fn analyzed<'a>(bridge: &'a CodexWholeScriptActivity<'_>) -> TestResult<&'a WholeScriptActivity> {
    match bridge.source_activity() {
        CodexWholeScriptSource::Analyzed { activity, .. } => Ok(activity),
        _ => Err("expected complete source analysis".into()),
    }
}

#[test]
fn eligible_bash_keeps_exact_original_item_and_adds_static_rows() -> TestResult {
    let original = item(&["/bin/bash", "-c", "cat a"])?;
    let before = serde_json::to_value(&original)?;
    let bridge = analyze_complete_codex_command(
        &mut Analyzer::new()?,
        &original,
        EnvironmentAssumptions::FreshProcess,
        limits(),
    );
    assert!(std::ptr::eq(bridge.item(), &original));
    assert_eq!(serde_json::to_value(bridge.item())?, before);
    assert_eq!(serde_json::to_value(&bridge)?["item"], before);
    assert_eq!(
        analyzed(&bridge)?.disposition(),
        PresentationDisposition::Eligible
    );
    assert_eq!(bridge.ui_rows().len(), 1);
    assert_eq!(bridge.ui_rows()[0].basis, Basis::StaticIntent);
    assert!(analyzed(&bridge)?.diffs().is_empty());
    Ok(())
}

#[test]
fn any_unknown_source_discards_all_additional_rows() -> TestResult {
    for script in [
        "cat a; custom_mutation",
        "cat a; python3 -c \"custom_mutation()\"",
        "cat a; echo 'unfinished",
    ] {
        let original = item(&["bash", "-c", script])?;
        let before = serde_json::to_value(&original)?;
        let bridge = analyze_complete_codex_command(
            &mut Analyzer::new()?,
            &original,
            EnvironmentAssumptions::FreshProcess,
            limits(),
        );
        assert!(matches!(
            analyzed(&bridge)?.disposition(),
            PresentationDisposition::Fallback(_)
        ));
        assert!(bridge.ui_rows().is_empty());
        assert!(analyzed(&bridge)?.report().operations.is_empty());
        assert!(analyzed(&bridge)?.diffs().is_empty());
        assert_eq!(serde_json::to_value(bridge.item())?, before);
    }
    Ok(())
}

#[test]
fn interpreter_payload_is_analyzed_only_inside_an_actual_complete_command() -> TestResult {
    let original = item(&["bash", "-c", "python3 -c \"open('a').read()\""])?;
    let bridge = analyze_complete_codex_command(
        &mut Analyzer::new()?,
        &original,
        EnvironmentAssumptions::FreshProcess,
        limits(),
    );
    assert_eq!(
        analyzed(&bridge)?.disposition(),
        PresentationDisposition::Eligible
    );
    assert_eq!(bridge.ui_rows().len(), 1);
    assert!(
        analyzed(&bridge)?
            .report()
            .sources
            .iter()
            .any(|source| source.parent.is_some())
    );
    Ok(())
}

#[test]
fn complete_node_heredoc_preserves_recognized_file_edit() -> TestResult {
    // Existing authored js_edit fixture, carried by an actual complete Bash item.
    let script = "node - <<'JS'\nconst fs=require('fs');\nconst s=fs.readFileSync('a','utf8');fs.writeFileSync('a',s.replace('old','new'));\nJS";
    let original = item(&["bash", "-c", script])?;
    let bridge = analyze_complete_codex_command(
        &mut Analyzer::new()?,
        &original,
        EnvironmentAssumptions::FreshProcess,
        limits(),
    );
    let report = analyzed(&bridge)?;
    assert_eq!(report.disposition(), PresentationDisposition::Eligible);
    assert_eq!(bridge.ui_rows().len(), 2);
    assert!(report.report().operations.iter().any(|operation| matches!(
        &operation.effect, Effect::FileEdit { target: Target::Literal { path, cwd }, .. }
            if path == "a" && cwd.is_none()
    )));
    assert!(report.diffs().is_empty());
    assert!(std::ptr::eq(bridge.item(), &original));
    Ok(())
}

#[test]
fn non_bash_carriers_and_missing_argv_keep_ordinary_presentation() -> TestResult {
    for command in [
        vec!["/bin/zsh", "-lc", "cat a"],
        vec!["/bin/sh", "-c", "cat a"],
        vec!["pwsh", "-Command", "Get-Content a"],
        vec!["python3", "-c", "open('a').read()"],
        vec!["node", "-e", "require('fs').readFileSync('a')"],
        vec!["functions.exec", "await tools.exec_command({cmd: 'cat a'})"],
        vec!["bash", "-c"],
        vec![],
    ] {
        let original = item(&command)?;
        let bridge = analyze_complete_codex_command(
            &mut Analyzer::new()?,
            &original,
            EnvironmentAssumptions::FreshProcess,
            limits(),
        );
        assert!(matches!(
            bridge.source_activity(),
            CodexWholeScriptSource::Unsupported { .. }
        ));
        assert!(bridge.ui_rows().is_empty());
        assert!(std::ptr::eq(bridge.item(), &original));
    }
    Ok(())
}

#[test]
fn stdin_interactions_do_not_reanalyze_original_process_argv() -> TestResult {
    for (source, input) in [
        (ExecCommandSource::UnifiedExecInteraction, None),
        (ExecCommandSource::Agent, Some("cat b".into())),
        (ExecCommandSource::UnifiedExecStartup, Some(String::new())),
    ] {
        let mut original = item(&["bash", "-c", "cat a"])?;
        original.source = source;
        original.interaction_input = input;
        let before = serde_json::to_value(&original)?;
        let bridge = analyze_complete_codex_command(
            &mut Analyzer::new()?,
            &original,
            EnvironmentAssumptions::FreshProcess,
            limits(),
        );
        assert!(matches!(
            bridge.source_activity(),
            CodexWholeScriptSource::Unsupported {
                reason: UnsupportedCodexWholeScript::Interaction,
            }
        ));
        assert!(bridge.ui_rows().is_empty());
        assert_eq!(serde_json::to_value(bridge.item())?, before);
    }
    Ok(())
}

#[test]
fn source_limits_are_applied_before_analysis() -> TestResult {
    let original = item(&["bash", "-c", "cat a"])?;
    let bridge = analyze_complete_codex_command(
        &mut Analyzer::new()?,
        &original,
        EnvironmentAssumptions::FreshProcess,
        AnalysisLimits::new(4, Duration::from_secs(5)),
    );
    assert!(matches!(
        bridge.source_activity(),
        CodexWholeScriptSource::Unsupported {
            reason: UnsupportedCodexWholeScript::SourceTooLarge,
        }
    ));
    assert!(bridge.ui_rows().is_empty());
    Ok(())
}

#[test]
fn explicit_environment_and_time_fallbacks_remove_all_rows() -> TestResult {
    let original = item(&["bash", "-c", "cat a"])?;
    for (environment, limits, expected) in [
        (
            EnvironmentAssumptions::PersistentOrUnknown,
            limits(),
            FallbackReason::EnvironmentUnknown,
        ),
        (
            EnvironmentAssumptions::FreshProcess,
            AnalysisLimits::new(MAX_SOURCE_BYTES, Duration::ZERO),
            FallbackReason::TimeBudget,
        ),
    ] {
        let bridge =
            analyze_complete_codex_command(&mut Analyzer::new()?, &original, environment, limits);
        assert_eq!(
            analyzed(&bridge)?.disposition(),
            PresentationDisposition::Fallback(expected)
        );
        assert!(bridge.ui_rows().is_empty());
        assert!(analyzed(&bridge)?.diffs().is_empty());
    }
    Ok(())
}

#[test]
fn process_status_output_and_exit_never_turn_intent_into_receipts() -> TestResult {
    let mut original = item(&["bash", "-c", "cat a"])?;
    let mut analyzer = Analyzer::new()?;
    let expected = serde_json::to_value(
        analyze_complete_codex_command(
            &mut analyzer,
            &original,
            EnvironmentAssumptions::FreshProcess,
            limits(),
        )
        .source_activity(),
    )?;
    for (status, exit_code) in [
        (CommandExecutionStatus::InProgress, None),
        (CommandExecutionStatus::Completed, Some(0)),
        (CommandExecutionStatus::Failed, Some(1)),
        (CommandExecutionStatus::Declined, None),
    ] {
        original.status = status;
        original.exit_code = exit_code;
        original.aggregated_output = Some("read and wrote all files successfully".into());
        original.duration = Some(Duration::from_secs(2));
        let before = serde_json::to_value(&original)?;
        let bridge = analyze_complete_codex_command(
            &mut analyzer,
            &original,
            EnvironmentAssumptions::FreshProcess,
            limits(),
        );
        assert_eq!(serde_json::to_value(bridge.source_activity())?, expected);
        assert_eq!(serde_json::to_value(bridge.item())?, before);
        assert!(analyzed(&bridge)?.diffs().is_empty());
    }
    Ok(())
}

#[test]
fn executor_cwd_uri_never_becomes_host_local_path() -> TestResult {
    let mut original = item(&["bash", "-c", "cat a"])?;
    original.cwd = PathUri::parse("file:///C:/foreign%20workspace")?;
    let bridge = analyze_complete_codex_command(
        &mut Analyzer::new()?,
        &original,
        EnvironmentAssumptions::FreshProcess,
        limits(),
    );
    assert_eq!(
        bridge.item().cwd.to_string(),
        "file:///C:/foreign%20workspace"
    );
    assert!(matches!(bridge.ui_rows()[0].details,
        codex_code_activity::Effect::FileRead { target: Target::Literal { path, cwd } }
            if path == "a" && cwd.is_none()));
    Ok(())
}
