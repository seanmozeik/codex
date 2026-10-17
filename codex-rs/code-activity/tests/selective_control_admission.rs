//! Authored source-only regression cases. Submitted programs are never executed.

use codex_code_activity::ActivityError;
use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::Language;
use codex_code_activity::SelectiveEditAnalysis;
use codex_code_activity::SelectiveEditDisposition;
use codex_code_activity::SelectiveEditInput;
use codex_code_activity::SelectiveLaterContext;
use codex_code_activity::SourceDescriptionFallback;
use codex_code_activity::Span;
use pretty_assertions::assert_eq;
use std::time::Duration;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn forecast(source: &str) -> Result<SelectiveEditAnalysis, ActivityError> {
    let input = SelectiveEditInput::from_source(
        "authored-control-admission".into(),
        Language::TypeScript,
        source,
        Some("/synthetic"),
        AnalysisLimits::new(32_768, Duration::from_secs(2)),
    )?;
    Ok(Analyzer::new()?.forecast_edits(input))
}

fn patch(name: &str) -> Result<String, serde_json::Error> {
    let payload = format!("*** Begin Patch\n*** Add File: {name}\n+authored\n*** End Patch\n");
    Ok(format!(
        "await tools.apply_patch({});",
        serde_json::to_string(&payload)?
    ))
}

fn text_at<'a>(analysis: &'a SelectiveEditAnalysis, evidence: &Span) -> Option<&'a str> {
    analysis
        .source_texts()
        .get(evidence.source_id.index())?
        .get(evidence.start_byte..evidence.end_byte)
}

fn assert_barrier(analysis: &SelectiveEditAnalysis, reason: &str, source: &str) {
    assert_eq!(analysis.disposition(), SelectiveEditDisposition::Analyzed);
    assert_eq!(analysis.barriers().len(), 1);
    assert_eq!(analysis.barriers()[0].reason, reason);
    assert_eq!(
        text_at(analysis, &analysis.barriers()[0].evidence),
        Some(source)
    );
    assert!(
        analysis
            .edits()
            .iter()
            .all(|edit| edit.later_context == SelectiveLaterContext::OpaqueSuffix)
    );
}

#[test]
fn ordinary_control_suffix_preserves_only_the_prior_operation_point() -> TestResult {
    let first = patch("first.txt")?;
    let later = patch("later.txt")?;
    for control in [
        format!("if(flag){{{later}}} else helper();"),
        format!("if({}){{helper();}}", later.trim_end_matches(';')),
        format!("for(let i=0;i<items.length;i++){{{later}}}"),
        format!("{{{later}}}"),
    ] {
        let analysis = forecast(&format!("{first}\n{control}\n{later}"))?;
        assert_eq!(analysis.edits().len(), 1, "{control}");
        assert_eq!(analysis.edits()[0].evidence.start_line, 1);
        assert!(analysis.edits()[0].predecessor_conditions.is_empty());
        assert_barrier(&analysis, "unsupportedControl", &control);
    }
    Ok(())
}

#[test]
fn ordinary_control_prefix_never_exposes_nested_or_later_edits() -> TestResult {
    let later = patch("later.txt")?;
    for control in [
        format!("if(false){{{later}}}"),
        format!("if({}){{helper();}}", later.trim_end_matches(';')),
        format!("if(true) helper(); else {{{later}}}"),
        format!("for(let i=0;i<0;i++){{{later}}}"),
        format!("{{{later}}}"),
    ] {
        let analysis = forecast(&format!("{control}\n{later}"))?;
        assert!(analysis.edits().is_empty(), "{control}");
        assert_barrier(&analysis, "unsupportedControl", &control);
    }
    Ok(())
}

#[test]
fn retained_operations_keep_predecessor_completion_conditions() -> TestResult {
    let first = patch("first.txt")?;
    let second = patch("second.txt")?;
    let control = "if(flag){helper();}";
    let analysis = forecast(&format!("{first}\n{second}\n{control}\n{second}"))?;
    assert_eq!(analysis.edits().len(), 2);
    assert_barrier(&analysis, "unsupportedControl", control);
    assert!(analysis.edits()[0].predecessor_conditions.is_empty());
    assert_eq!(analysis.edits()[1].predecessor_conditions.len(), 1);
    assert_eq!(
        analysis.edits()[1].predecessor_conditions[0],
        analysis.edits()[0].evidence
    );
    Ok(())
}

#[test]
fn nested_lexical_host_names_do_not_shadow_an_earlier_module_operation() -> TestResult {
    let first = patch("first.txt")?;
    for control in [
        "{const tools=helper; const text=helper; text(tools);}",
        "if(false){const tools=helper;} else {let text=helper;}",
        "for(let tools=0;tools<2;tools++){const tools=helper;}",
        "{const i=0; {const i=1;} if(flag){const i=2;}}",
    ] {
        let analysis = forecast(&format!("{first}\n{control}"))?;
        assert_eq!(analysis.edits().len(), 1, "{control}");
        assert_barrier(&analysis, "unsupportedControl", control);
    }
    Ok(())
}

#[test]
fn whole_module_shadowing_and_hidden_hoists_still_refuse_every_operation() -> TestResult {
    for suffix in [
        "if(false){} const tools=helper;",
        "{helper();} let text=helper;",
        "if(false){var tools;}",
        "if(true){} else {var tools;}",
        "for(let i=0;i<0;i++){var tools;}",
        "{if(false){function text(){}}}",
        "if(false){class tools{}}",
    ] {
        assert_refused(suffix)?;
    }
    Ok(())
}

fn assert_refused(suffix: &str) -> TestResult {
    let analysis = forecast(&format!("{}\n{suffix}", patch("first.txt")?))?;
    assert_eq!(
        analysis.disposition(),
        SelectiveEditDisposition::Unavailable(SourceDescriptionFallback::UnsupportedSource),
        "{suffix}"
    );
    assert!(analysis.edits().is_empty(), "{suffix}");
    assert!(analysis.sources().is_empty(), "{suffix}");
    assert!(analysis.source_texts().is_empty(), "{suffix}");
    assert!(analysis.barriers().is_empty(), "{suffix}");
    Ok(())
}

#[test]
fn dead_branches_and_loops_do_not_hide_early_errors_or_malformed_syntax() -> TestResult {
    for suffix in [
        "{let same=0;const same=1;}",
        "if(false){const same=0;let same=1;}",
        "if(true){} else {let same=0;let same=1;}",
        "for(let i=0;i<0;i++){const same=0;const same=1;}",
        "if(false){const eval=0;}",
        "for(let arguments=0;arguments<0;arguments++){}",
        "if(false) const value=0;",
        "for(let i=0;i<0;i++) let value=0;",
        "if(false){const missing=;}",
        "for(let i=0;i<0;i++){",
        "if(flag){} else",
        "{const value:number=0;}",
        "if(false){return 0;}",
        "for(let i=0;i<0;eval++){}",
        "if(false){const value=1;}\u{0085}",
    ] {
        assert_refused(suffix)?;
    }
    Ok(())
}

#[test]
fn forms_outside_the_finite_control_grammar_remain_whole_source_refusals() -> TestResult {
    for suffix in [
        "for(;;){}",
        "for(const item of items){}",
        "while(flag){helper();}",
        "try{helper();}catch(error){}",
        "if(flag && other){helper();}",
        "if(flag){globalThis['text'](1);}",
        "{const value=await result;}",
    ] {
        assert_refused(suffix)?;
    }
    Ok(())
}

#[test]
fn admitted_expression_shapes_remain_opaque_without_host_trust() -> TestResult {
    let first = patch("first.txt")?;
    let later = patch("later.txt")?;
    for expression in [
        "const value=left+right;",
        "const value=left??right;",
        "const value=left<right;",
        "const value=left===right;",
        "const value=items[0];",
        "index++;",
        "const value={item};",
    ] {
        let analysis = forecast(&format!("{first}\n{expression}\n{later}"))?;
        assert_eq!(analysis.edits().len(), 1, "{expression}");
        assert_barrier(&analysis, "unknownExecution", expression);
        let before = forecast(&format!("{expression}\n{later}"))?;
        assert!(before.edits().is_empty(), "{expression}");
        assert_barrier(&before, "unknownExecution", expression);
    }
    Ok(())
}

#[test]
fn ordinary_display_completion_remains_unknown_even_with_valid_control_suffix() -> TestResult {
    let first = patch("first.txt")?;
    let display = format!("text({});", first.trim_end_matches(';'));
    let later = patch("later.txt")?;
    let analysis = forecast(&format!("{display}\n{later}\nif(flag){{helper();}}"))?;
    assert_eq!(analysis.edits().len(), 1);
    assert_barrier(&analysis, "unknownExecution", &display);
    Ok(())
}

#[test]
fn earlier_unknown_execution_is_not_bypassed_by_newly_admitted_control() -> TestResult {
    let later = patch("later.txt")?;
    let analysis = forecast(&format!("unknown(); if(false){{{later}}}\n{later}"))?;
    assert!(analysis.edits().is_empty());
    assert_barrier(&analysis, "unknownExecution", "unknown();");
    Ok(())
}
