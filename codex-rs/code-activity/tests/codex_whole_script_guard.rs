//! Authored command-item fixtures; submitted source is never executed.

use codex_code_activity::AnalysisLimits;
use codex_code_activity::Basis;
use codex_code_activity::CodexWholeScriptSource;
use codex_code_activity::Effect;
use codex_code_activity::EnvironmentAssumptions;
use codex_code_activity::GUARD_MAX_CALL_ID_BYTES;
use codex_code_activity::GUARD_MAX_SOURCE_BYTES;
use codex_code_activity::GUARD_MAX_WAIT;
use codex_code_activity::GuardDisposition;
use codex_code_activity::GuardFallback;
use codex_code_activity::Target;
use codex_code_activity::UnsupportedCodexWholeScript;
use codex_code_activity::analyze_guarded_codex_command;
use codex_protocol::items::CommandExecutionItem;
use codex_protocol::items::CommandExecutionStatus;
use codex_protocol::protocol::ExecCommandSource;
use codex_shell_command::parse_command::parse_command;
use codex_utils_path_uri::PathUri;
use pretty_assertions::assert_eq;
use std::time::Duration;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn item(source: &str) -> Result<CommandExecutionItem, Box<dyn std::error::Error>> {
    let command = vec!["/bin/bash".into(), "-c".into(), source.into()];
    Ok(CommandExecutionItem {
        sandbox_type: None,
        model_context: None,
        id: "guarded-command".into(),
        plugin_id: Some("authored-fixture".into()),
        script_path: None,
        process_id: Some("fixture-process".into()),
        parsed_cmd: parse_command(&command),
        command,
        cwd: PathUri::parse("file:///C:/foreign%20workspace")?,
        source: ExecCommandSource::UnifiedExecStartup,
        interaction_input: None,
        status: CommandExecutionStatus::Completed,
        aggregated_output: Some("ordinary output retained".into()),
        exit_code: Some(0),
        duration: Some(Duration::from_millis(12)),
    })
}

#[test]
fn guarded_actual_item_retains_original_and_adds_only_static_intent() -> TestResult {
    let original = item("cat a")?;
    let before = serde_json::to_value(&original)?;
    let bridge = analyze_guarded_codex_command(
        &original,
        EnvironmentAssumptions::FreshProcess,
        AnalysisLimits::default(),
        GUARD_MAX_WAIT,
    );
    assert!(std::ptr::eq(bridge.item(), &original));
    assert_eq!(serde_json::to_value(bridge.item())?, before);
    assert_eq!(serde_json::to_value(&bridge)?["item"], before);
    let CodexWholeScriptSource::Guarded { shell, activity } = bridge.source_activity() else {
        return Err("expected guarded source result".into());
    };
    assert_eq!(*shell, "/bin/bash");
    assert_eq!(activity.disposition(), GuardDisposition::Eligible);
    assert!(activity.diffs().is_empty());
    let rows = bridge.ui_rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].basis, Basis::StaticIntent);
    assert!(matches!(rows[0].details,
        Effect::FileRead { target: Target::Literal { path, cwd } }
            if path == "a" && cwd.is_none()));
    Ok(())
}

#[test]
fn guard_environment_and_zero_wait_fallback_keep_original_item() -> TestResult {
    for (environment, wait, expected) in [
        (
            EnvironmentAssumptions::PersistentOrUnknown,
            GUARD_MAX_WAIT,
            GuardFallback::EnvironmentUnknown,
        ),
        (
            EnvironmentAssumptions::FreshProcess,
            Duration::ZERO,
            GuardFallback::Deadline,
        ),
    ] {
        let original = item("cat a")?;
        let before = serde_json::to_value(&original)?;
        let bridge =
            analyze_guarded_codex_command(&original, environment, AnalysisLimits::default(), wait);
        assert!(std::ptr::eq(bridge.item(), &original));
        assert_eq!(serde_json::to_value(bridge.item())?, before);
        assert!(bridge.ui_rows().is_empty());
        let CodexWholeScriptSource::Guarded { activity, .. } = bridge.source_activity() else {
            return Err("expected guarded fallback".into());
        };
        assert_eq!(activity.disposition(), GuardDisposition::Fallback(expected));
        assert!(activity.eligible_activity().is_none());
        assert!(activity.diffs().is_empty());
    }
    Ok(())
}

#[test]
fn guard_rejects_source_and_identity_caps_before_submission() -> TestResult {
    let oversized_source = item(&" ".repeat(GUARD_MAX_SOURCE_BYTES + 1))?;
    let mut oversized_id = item("cat a")?;
    oversized_id.id = "é".repeat(GUARD_MAX_CALL_ID_BYTES / 2 + 1);
    for (original, limits, expected) in [
        (
            oversized_source,
            AnalysisLimits::default(),
            UnsupportedCodexWholeScript::SourceTooLarge,
        ),
        (
            oversized_id,
            AnalysisLimits::default(),
            UnsupportedCodexWholeScript::IdentityTooLarge,
        ),
        (
            item("cat a")?,
            AnalysisLimits::new(4, GUARD_MAX_WAIT),
            UnsupportedCodexWholeScript::SourceTooLarge,
        ),
    ] {
        let before = serde_json::to_value(&original)?;
        let bridge = analyze_guarded_codex_command(
            &original,
            EnvironmentAssumptions::FreshProcess,
            limits,
            GUARD_MAX_WAIT,
        );
        assert!(bridge.ui_rows().is_empty());
        assert!(std::ptr::eq(bridge.item(), &original));
        assert_eq!(serde_json::to_value(bridge.item())?, before);
        assert!(matches!(bridge.source_activity(),
            CodexWholeScriptSource::Unsupported { reason } if *reason == expected));
    }
    Ok(())
}

#[test]
fn guard_preserves_carrier_and_interaction_abstention() -> TestResult {
    let mut interaction = item("cat a")?;
    interaction.source = ExecCommandSource::UnifiedExecInteraction;
    let mut stdin = item("cat a")?;
    stdin.interaction_input = Some(String::new());
    let mut zsh = item("cat a")?;
    zsh.command[0] = "/bin/zsh".into();
    let mut incomplete = item("cat a")?;
    let _ = incomplete.command.pop();
    for (original, expected) in [
        (interaction, UnsupportedCodexWholeScript::Interaction),
        (stdin, UnsupportedCodexWholeScript::Interaction),
        (zsh, UnsupportedCodexWholeScript::UnsupportedShell),
        (incomplete, UnsupportedCodexWholeScript::UnrecognizedArgv),
    ] {
        let before = serde_json::to_value(&original)?;
        let bridge = analyze_guarded_codex_command(
            &original,
            EnvironmentAssumptions::FreshProcess,
            AnalysisLimits::default(),
            GUARD_MAX_WAIT,
        );
        assert!(bridge.ui_rows().is_empty());
        assert_eq!(serde_json::to_value(bridge.item())?, before);
        assert!(matches!(bridge.source_activity(),
            CodexWholeScriptSource::Unsupported { reason } if *reason == expected));
    }
    Ok(())
}
