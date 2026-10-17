//! Complete-source presentation boundaries; submitted programs remain inert.

use codex_code_activity::ActivityError;
use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::CompleteScript;
use codex_code_activity::EnvironmentAssumptions;
use codex_code_activity::FallbackReason;
use codex_code_activity::FileSnapshot;
use codex_code_activity::Language;
use codex_code_activity::MAX_SOURCE_BYTES;
use codex_code_activity::PresentationDisposition;
use codex_code_activity::Request;
use codex_code_activity::ScriptBuffer;
use codex_code_activity::WholeScriptActivity;
use pretty_assertions::assert_eq;
use std::time::Duration;

fn complete(source: &str) -> Result<WholeScriptActivity, ActivityError> {
    let request =
        Request::new("whole".into(), Language::Python, source.into(), None).with_snapshots(vec![
            FileSnapshot::new("a".into(), Some("before".into()), Some("after".into())),
        ]);
    let input = CompleteScript::from_request(
        request,
        EnvironmentAssumptions::FreshProcess,
        AnalysisLimits::new(MAX_SOURCE_BYTES, Duration::from_secs(5)),
    )?;
    Ok(Analyzer::new()?.analyze_complete(input))
}

fn assert_fallback(activity: &WholeScriptActivity) {
    assert!(matches!(
        activity.disposition(),
        PresentationDisposition::Fallback(_)
    ));
    assert_eq!(activity.ui_rows().len(), 0);
    assert_eq!(activity.report().operations.len(), 0);
    assert_eq!(activity.diffs().len(), 0);
    assert_eq!(activity.report().diffs.len(), 0);
}

#[test]
fn supported_complete_script_exposes_intent_and_host_diff() -> Result<(), ActivityError> {
    let activity = complete("open('a').read()")?;
    assert_eq!(activity.disposition(), PresentationDisposition::Eligible);
    assert_eq!(activity.ui_rows().len(), 1);
    assert_eq!(activity.diffs().len(), 1);
    Ok(())
}

#[test]
fn unknown_after_known_read_discards_all_rows_and_diffs() -> Result<(), ActivityError> {
    assert_fallback(&complete("open('a').read()\ncustom_mutation()")?);
    Ok(())
}

#[test]
fn unknown_before_known_read_discards_all_rows_and_diffs() -> Result<(), ActivityError> {
    assert_fallback(&complete("custom_mutation()\nopen('a').read()")?);
    Ok(())
}

#[test]
fn unknown_target_discards_known_sibling_and_diff() -> Result<(), ActivityError> {
    assert_fallback(&complete("open('a').read()\nopen(dynamic_path).read()")?);
    Ok(())
}

#[test]
fn unknown_mode_discards_known_sibling_and_diff() -> Result<(), ActivityError> {
    assert_fallback(&complete(
        "open('a').read()\nopen('b', dynamic_mode).write('x')",
    )?);
    Ok(())
}

#[test]
fn late_syntax_error_discards_known_prefix_and_diff() -> Result<(), ActivityError> {
    assert_fallback(&complete("open('a').read()\nif (")?);
    Ok(())
}

#[test]
fn nested_unknown_source_discards_outer_recognition_and_diff() -> Result<(), ActivityError> {
    assert_fallback(&complete("open('a').read()\nexec(\"custom_mutation()\")")?);
    Ok(())
}

#[test]
fn standard_module_initialization_uncertainty_is_not_waived() -> Result<(), ActivityError> {
    assert_fallback(&complete(
        "from pathlib import Path\nPath('a').read_text()",
    )?);
    Ok(())
}

#[test]
fn operation_limit_discards_entire_report() -> Result<(), ActivityError> {
    assert_fallback(&complete(&"open('a').read()\n".repeat(300))?);
    Ok(())
}

#[test]
fn buffer_emits_nothing_and_only_final_input_is_analyzed() -> Result<(), ActivityError> {
    let mut buffer = ScriptBuffer::new(
        "buffer".into(),
        Language::Python,
        None,
        EnvironmentAssumptions::FreshProcess,
        AnalysisLimits::default(),
    );
    buffer.append("open('a').read()\n")?;
    buffer.append("custom_mutation()")?;
    let activity = Analyzer::new()?.analyze_complete(buffer.finish()?);
    assert_fallback(&activity);
    Ok(())
}

#[test]
fn byte_overflow_permanently_aborts_buffer() -> Result<(), ActivityError> {
    let mut buffer = ScriptBuffer::new(
        "buffer".into(),
        Language::Python,
        None,
        EnvironmentAssumptions::FreshProcess,
        AnalysisLimits::new(4, Duration::from_secs(1)),
    );
    buffer.append("é")?;
    assert!(matches!(
        buffer.append("abc"),
        Err(ActivityError::SourceLimit)
    ));
    assert!(matches!(
        buffer.append(""),
        Err(ActivityError::StreamAborted)
    ));
    assert!(matches!(buffer.finish(), Err(ActivityError::StreamAborted)));
    Ok(())
}

#[test]
fn complete_input_cannot_bypass_byte_limit() {
    let input = CompleteScript::from_request(
        Request::new("large".into(), Language::Python, "pass".into(), None),
        EnvironmentAssumptions::FreshProcess,
        AnalysisLimits::new(3, Duration::from_secs(1)),
    );
    assert!(matches!(input, Err(ActivityError::SourceLimit)));
}

#[test]
fn persistent_environment_forces_fallback_even_for_literal_builtin_read()
-> Result<(), ActivityError> {
    let input = CompleteScript::from_request(
        Request::new(
            "persistent".into(),
            Language::Python,
            "open('a').read()".into(),
            None,
        ),
        EnvironmentAssumptions::PersistentOrUnknown,
        AnalysisLimits::default(),
    )?;
    let activity = Analyzer::new()?.analyze_complete(input);
    assert_eq!(
        activity.disposition(),
        PresentationDisposition::Fallback(FallbackReason::EnvironmentUnknown)
    );
    assert_fallback(&activity);
    Ok(())
}

#[test]
fn expired_budget_falls_back_and_does_not_poison_next_analysis() -> Result<(), ActivityError> {
    let input = CompleteScript::from_request(
        Request::new(
            "timeout".into(),
            Language::Python,
            "open('a').read()".into(),
            None,
        ),
        EnvironmentAssumptions::FreshProcess,
        AnalysisLimits::new(MAX_SOURCE_BYTES, Duration::ZERO),
    )?;
    let mut analyzer = Analyzer::new()?;
    let activity = analyzer.analyze_complete(input);
    assert_eq!(
        activity.disposition(),
        PresentationDisposition::Fallback(FallbackReason::TimeBudget)
    );
    assert_fallback(&activity);
    let baseline = analyzer.analyze(Request::new(
        "next".into(),
        Language::Python,
        "open('next').read()".into(),
        None,
    ));
    assert_eq!(baseline.operations.len(), 1);
    Ok(())
}

#[test]
fn baseline_still_preserves_partial_diagnostic_evidence() -> Result<(), ActivityError> {
    let baseline = Analyzer::new()?.analyze(Request::new(
        "baseline".into(),
        Language::Python,
        "open('a').read()\ncustom_mutation()".into(),
        None,
    ));
    assert_eq!(baseline.operations.len(), 1);
    assert!(!baseline.unresolved.is_empty());
    Ok(())
}

#[test]
fn snapshot_budget_discards_rows_and_all_other_diffs() -> Result<(), ActivityError> {
    let input = CompleteScript::from_request(
        Request::new(
            "snapshots".into(),
            Language::Python,
            "open('a').read()".into(),
            None,
        )
        .with_snapshots(vec![
            FileSnapshot::new("a".into(), Some("before".into()), Some("after".into())),
            FileSnapshot::new("large".into(), None, Some("x".repeat(262_145))),
        ]),
        EnvironmentAssumptions::FreshProcess,
        AnalysisLimits::new(MAX_SOURCE_BYTES, Duration::from_secs(5)),
    )?;
    let activity = Analyzer::new()?.analyze_complete(input);
    assert_eq!(
        activity.disposition(),
        PresentationDisposition::Fallback(FallbackReason::SnapshotBudget)
    );
    assert_fallback(&activity);
    Ok(())
}

#[test]
fn python_assertion_message_cannot_add_unreachable_read() -> Result<(), ActivityError> {
    assert_fallback(&complete("assert True, open('never').read()")?);
    Ok(())
}

#[test]
fn python_comparison_chain_cannot_add_unreachable_read() -> Result<(), ActivityError> {
    assert_fallback(&complete("0 > 1 < open('never').read()")?);
    Ok(())
}

#[test]
fn python_assignment_annotation_cannot_silently_omit_possible_read() -> Result<(), ActivityError> {
    assert_fallback(&complete("x: open('a').read() = 1")?);
    Ok(())
}

#[test]
fn python_return_annotation_cannot_silently_omit_possible_read() -> Result<(), ActivityError> {
    assert_fallback(&complete("def f() -> open('a').read():\n    pass")?);
    Ok(())
}

#[test]
fn typescript_complex_assignment_targets_cannot_hide_effects() -> Result<(), ActivityError> {
    for source in [
        "require('fs').readFileSync('a'); require('fs').unlinkSync('b').x = 1;",
        "require('fs').readFileSync('a'); custom_mutation().x = 1;",
        "require('fs').readFileSync('a'); object[custom_mutation()] = 1;",
        "require('fs').readFileSync('a'); object[custom_mutation()] += 1;",
        "require('fs').readFileSync('a'); object[custom_mutation()]++;",
        "require('fs').readFileSync('a'); [object.x] = [1];",
    ] {
        let request = Request::new("target".into(), Language::TypeScript, source.into(), None)
            .with_snapshots(vec![FileSnapshot::new(
                "a".into(),
                None,
                Some("after".into()),
            )]);
        let input = CompleteScript::from_request(
            request,
            EnvironmentAssumptions::FreshProcess,
            AnalysisLimits::new(MAX_SOURCE_BYTES, Duration::from_secs(5)),
        )?;
        let activity = Analyzer::new()?.analyze_complete(input);
        assert_fallback(&activity);
    }
    Ok(())
}

#[test]
fn python_positional_open_options_cannot_hide_callbacks_or_codecs() -> Result<(), ActivityError> {
    for source in [
        "def opener(path, flags):\n    open('b', 'w')\n    return 0\nopen('a', 'r', 1, None, None, None, True, opener).read()",
        "open('a', 'r', 1, 'custom_codec').read()",
        "alias = open\nalias('a', 'r', 1, 'custom_codec').read()",
    ] {
        assert_fallback(&complete(source)?);
    }
    Ok(())
}

#[test]
fn deno_runtime_and_aliases_discard_all_rows_and_diffs() -> Result<(), ActivityError> {
    for source in [
        "require('fs').readFileSync('a'); Deno.writeTextFileSync('a','x',{createNew:true});",
        "const options={createNew:true}; const writer=Deno.writeTextFileSync; writer('a','x',options);",
        "const runtime={Deno}; runtime.Deno.writeTextFileSync('a','x',{createNew:true});",
    ] {
        let request = Request::new("deno".into(), Language::TypeScript, source.into(), None)
            .with_snapshots(vec![FileSnapshot::new(
                "a".into(),
                Some("before".into()),
                Some("after".into()),
            )]);
        let input = CompleteScript::from_request(
            request,
            EnvironmentAssumptions::FreshProcess,
            AnalysisLimits::new(MAX_SOURCE_BYTES, Duration::from_secs(5)),
        )?;
        assert_fallback(&Analyzer::new()?.analyze_complete(input));
    }
    Ok(())
}
