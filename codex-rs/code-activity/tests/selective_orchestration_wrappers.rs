//! Authored parser-input controls, independently reviewed before measurement.
//! No fixture program is executed.

use codex_code_activity::ActivityError;
use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::Language;
use codex_code_activity::SelectiveCarrierBasis;
use codex_code_activity::SelectiveEditAnalysis;
use codex_code_activity::SelectiveEditBasis;
use codex_code_activity::SelectiveEditDetails;
use codex_code_activity::SelectiveEditDisposition;
use codex_code_activity::SelectiveEditInput;
use codex_code_activity::SelectiveLaterContext;
use codex_code_activity::SourceDescriptionFallback;
use codex_code_activity::Span;
use pretty_assertions::assert_eq;
use std::time::Duration;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const PATCH: &str = "*** Begin Patch\n*** Delete File: public.txt\n*** End Patch";

fn forecast(source: &str) -> Result<SelectiveEditAnalysis, ActivityError> {
    let input = SelectiveEditInput::from_source(
        "authored-wrapper-v3".into(),
        Language::TypeScript,
        source,
        Some("/workspace"),
        AnalysisLimits::new(32_768, Duration::from_secs(1)),
    )?;
    Ok(Analyzer::new()?.forecast_edits(input))
}

fn patch_call() -> Result<String, serde_json::Error> {
    Ok(format!(
        "tools.apply_patch({})",
        serde_json::to_string(PATCH)?
    ))
}

fn text_at<'a>(analysis: &'a SelectiveEditAnalysis, evidence: &Span) -> Option<&'a str> {
    analysis
        .source_texts()
        .get(evidence.source_id.index())?
        .get(evidence.start_byte..evidence.end_byte)
}

fn assert_consumer_stop(analysis: &SelectiveEditAnalysis, statement: &str) {
    assert_eq!(analysis.disposition(), SelectiveEditDisposition::Analyzed);
    assert_eq!(analysis.barriers().len(), 1);
    assert_eq!(analysis.barriers()[0].reason, "unknownExecution");
    assert_eq!(
        text_at(analysis, &analysis.barriers()[0].evidence),
        Some(statement)
    );
    assert!(
        analysis
            .edits()
            .iter()
            .all(|edit| edit.later_context == SelectiveLaterContext::OpaqueSuffix)
    );
}

#[test]
fn awaited_patch_keeps_inner_evidence_and_stops_after_consumer() -> TestResult {
    let call = patch_call()?;
    let statement = format!("text(await {call});");
    let source = format!("/*é*/\n{statement}\nawait {call};");
    let analysis = forecast(&source)?;
    assert_eq!(analysis.edits().len(), 1);
    assert_consumer_stop(&analysis, &statement);
    let edit = &analysis.edits()[0];
    assert_eq!(edit.basis, SelectiveEditBasis::HostToolRequest);
    assert_eq!(text_at(&analysis, &edit.evidence), Some(call.as_str()));
    assert_eq!(edit.evidence.start_line, 2);
    assert!(edit.predecessor_conditions.is_empty());
    assert!(edit.carrier_assumptions.is_empty());
    let SelectiveEditDetails::Patch { patch, cwd } = &edit.details else {
        return Err("expected a literal patch request".into());
    };
    assert_eq!(patch.payload, PATCH);
    assert_eq!(patch.files.len(), 1);
    assert_eq!(cwd.as_deref(), Some("/workspace"));
    assert_eq!(text_at(&analysis, &patch.invocation), Some(call.as_str()));
    let literal = serde_json::to_string(PATCH)?;
    assert_eq!(text_at(&analysis, &patch.argument), Some(literal.as_str()));
    Ok(())
}

#[test]
fn transparent_argument_parentheses_and_initializer_keep_the_owner_barrier() -> TestResult {
    let call = patch_call()?;
    for statement in [
        format!("text((await ({call})));"),
        format!("(text(await {call}));"),
        format!("const displayed = text(await {call});"),
        format!("let displayed = text(await {call});"),
    ] {
        let analysis = forecast(&statement)?;
        assert_eq!(analysis.edits().len(), 1, "{statement}");
        assert_consumer_stop(&analysis, &statement);
        assert_eq!(
            text_at(&analysis, &analysis.edits()[0].evidence),
            Some(call.as_str())
        );
    }
    Ok(())
}

#[test]
fn nested_python_operations_precede_consumer_without_promoting_successors() -> TestResult {
    let python = "open(\"child.txt\", \"w\").write(\"body\")";
    let shell = format!("python3 -c '{python}'");
    let call = format!(
        "tools.exec_command({{cmd: {}}})",
        serde_json::to_string(&shell)?
    );
    let statement = format!("text(await {call});");
    let source = format!("{statement}\nawait {};", patch_call()?);
    let analysis = forecast(&source)?;
    assert_eq!(analysis.sources().len(), 3);
    assert_eq!(analysis.source_texts()[1], shell);
    assert_eq!(analysis.source_texts()[2], python);
    assert_eq!(analysis.edits().len(), 2);
    assert_consumer_stop(&analysis, &statement);
    for edit in analysis.edits() {
        assert_eq!(edit.basis, SelectiveEditBasis::StandardApiForecast);
        assert_eq!(edit.evidence.source_id.index(), 2);
        assert!(edit.predecessor_conditions.is_empty());
        assert_eq!(edit.carrier_assumptions.len(), 3);
        assert_eq!(
            edit.carrier_assumptions[0].basis,
            SelectiveCarrierBasis::StandardShellDispatchAndStartup
        );
        assert_eq!(
            edit.carrier_assumptions[1].basis,
            SelectiveCarrierBasis::StandardShellDispatchAndStartup
        );
        assert_eq!(
            edit.carrier_assumptions[2].basis,
            SelectiveCarrierBasis::StandardPython3Cli
        );
    }
    Ok(())
}

#[test]
fn modeled_prefix_conditions_are_preserved_and_suffix_patch_is_not_recovered() -> TestResult {
    let call = patch_call()?;
    let prefix = format!("await {call};");
    let wrapper = format!("text(await {call});");
    let analysis = forecast(&format!("{prefix}\n{wrapper}\nawait {call};"))?;
    assert_eq!(analysis.edits().len(), 2);
    assert_consumer_stop(&analysis, &wrapper);
    assert!(analysis.edits()[0].predecessor_conditions.is_empty());
    assert_eq!(analysis.edits()[1].predecessor_conditions.len(), 1);
    assert_eq!(
        text_at(&analysis, &analysis.edits()[1].predecessor_conditions[0]),
        Some(call.as_str())
    );
    assert!(
        analysis.edits()[1].predecessor_conditions[0].end_byte
            < analysis.edits()[1].evidence.start_byte
    );
    Ok(())
}

#[test]
fn direct_result_binding_and_text_result_behavior_remain_unchanged() -> TestResult {
    let call = patch_call()?;
    let statement = "text(result);";
    let source = format!("const result = await {call};\n{statement}\nawait {call};");
    let analysis = forecast(&source)?;
    assert_eq!(analysis.edits().len(), 1);
    assert_consumer_stop(&analysis, statement);
    assert_eq!(
        text_at(&analysis, &analysis.edits()[0].evidence),
        Some(call.as_str())
    );
    Ok(())
}

#[test]
fn arbitrary_callee_and_argument_shapes_never_expose_nested_tool_events() -> TestResult {
    let call = patch_call()?;
    for source in [
        format!("unknown(await {call});"),
        format!("consumer.text(await {call});"),
        format!("alias(await {call});"),
        format!("(text)(await {call});"),
        format!("text(await {call}, null);"),
        format!("text({call});"),
        format!("text([await {call}]);"),
        format!("text({{value: await {call}}});"),
        format!("text(JSON.stringify(await {call}));"),
        format!("await text(await {call});"),
        format!("later(await {call}); const later = text;"),
        "text(await tools.exec_command({cmd: dynamic}));".to_owned(),
        "text(await tools.exec_command({cmd: 'echo inert', workdir: 'relative'}));".to_owned(),
        "text(await tools.apply_patch(payload)); const payload = 'unused';".to_owned(),
        "text(await tools.unknown({cmd: 'echo inert'}));".to_owned(),
    ] {
        let analysis = forecast(&source)?;
        assert!(analysis.edits().is_empty(), "{source}");
        assert!(!analysis.barriers().is_empty(), "{source}");
    }
    Ok(())
}

#[test]
fn opaque_predecessors_never_become_reachability_conditions() -> TestResult {
    let call = patch_call()?;
    for prefix in [
        "unknown();",
        "text = replacement;",
        "tools = replacement;",
        "tools.apply_patch = replacement;",
        "const alias = text;",
        "text(result);",
        "const early = later; const later = 'data';",
    ] {
        let analysis = forecast(&format!("{prefix}\ntext(await {call});"))?;
        assert!(analysis.edits().is_empty(), "{prefix}");
        assert!(!analysis.barriers().is_empty(), "{prefix}");
    }
    Ok(())
}

#[test]
fn whole_module_shadowing_and_invalid_suffix_erase_every_forecast() -> TestResult {
    let call = patch_call()?;
    for suffix in [
        "const text = 'later';",
        "let tools = 'later';",
        "const same = 1; const same = 2;",
        "return 1;",
        "const typed: string = 'later';",
        "const f = () => 1;",
        "if (true) { var tools; }",
    ] {
        let analysis = forecast(&format!("text(await {call});\n{suffix}"))?;
        assert_eq!(
            analysis.disposition(),
            SelectiveEditDisposition::Unavailable(SourceDescriptionFallback::UnsupportedSource)
        );
        assert!(analysis.edits().is_empty());
        assert!(analysis.sources().is_empty());
        assert!(analysis.source_texts().is_empty());
        assert!(analysis.barriers().is_empty());
    }
    Ok(())
}

#[test]
fn unresolved_inner_patch_stays_unresolved_instead_of_becoming_a_wrapper_success() -> TestResult {
    let source = "text(await tools.apply_patch('not a valid patch'));";
    let analysis = forecast(source)?;
    assert_eq!(analysis.disposition(), SelectiveEditDisposition::Analyzed);
    assert!(analysis.edits().is_empty());
    assert_eq!(analysis.barriers().len(), 1);
    assert_eq!(analysis.barriers()[0].reason, "unresolvedPatch");
    assert_eq!(
        text_at(&analysis, &analysis.barriers()[0].evidence),
        Some("tools.apply_patch('not a valid patch')")
    );
    Ok(())
}

#[test]
fn both_wrapper_events_charge_existing_bounded_plan_capacity() -> TestResult {
    let call = patch_call()?;
    let source = format!("text(await {call});\n").repeat(129);
    let analysis = forecast(&source)?;
    assert_eq!(
        analysis.disposition(),
        SelectiveEditDisposition::Unavailable(SourceDescriptionFallback::SourceLimit)
    );
    assert!(analysis.edits().is_empty());
    assert!(analysis.sources().is_empty());
    assert!(analysis.barriers().is_empty());
    Ok(())
}

#[test]
fn noncall_await_operands_remain_outside_the_unchanged_full_module_gate() -> TestResult {
    let call = patch_call()?;
    for argument in ["1", "result", "result.output"] {
        let source = format!("await {call}; text(await {argument});");
        let analysis = forecast(&source)?;
        assert_eq!(
            analysis.disposition(),
            SelectiveEditDisposition::Unavailable(SourceDescriptionFallback::UnsupportedSource)
        );
        assert!(analysis.edits().is_empty());
        assert!(analysis.sources().is_empty());
        assert!(analysis.barriers().is_empty());
    }
    Ok(())
}
