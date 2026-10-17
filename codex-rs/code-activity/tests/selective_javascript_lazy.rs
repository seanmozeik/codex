//! Authored JavaScript is parsed only; no represented command or file is opened.

use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::CompleteScript;
use codex_code_activity::EnvironmentAssumptions;
use codex_code_activity::FileSnapshot;
use codex_code_activity::Language;
use codex_code_activity::PresentationDisposition;
use codex_code_activity::Request;
use codex_code_activity::SelectiveEditInput;
use pretty_assertions::assert_eq;
use serde_json::Value;
use std::time::Duration;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const OPEN: &str = "const fs=require('fs'); const fd=fs.openSync('out','r+');";
const WRITE: &str = "fs.writeSync(fd,'X');";

fn forecast(source: &str) -> TestResult<Value> {
    let input = SelectiveEditInput::from_node_commonjs(
        "authored-lazy".into(),
        source,
        Some("/authored"),
        AnalysisLimits::new(32_768, Duration::from_secs(2)),
    )?;
    Ok(serde_json::to_value(
        Analyzer::new()?.forecast_edits(input),
    )?)
}

fn array(value: &Value) -> TestResult<&Vec<Value>> {
    value.as_array().ok_or_else(|| "missing JSON array".into())
}

fn prediction(report: &Value, ordinal: usize) -> &Value {
    &report["edits"][ordinal]["details"]["prediction"]
}

fn no_barriers(report: &Value) -> bool {
    report
        .get("javascriptBarriers")
        .is_none_or(|barriers| barriers.as_array().is_some_and(Vec::is_empty))
}

fn span_text<'a>(source: &'a str, span: &Value) -> TestResult<&'a str> {
    let start = usize::try_from(span["startByte"].as_u64().ok_or("missing start")?)?;
    let end = usize::try_from(span["endByte"].as_u64().ok_or("missing end")?)?;
    source.get(start..end).ok_or_else(|| "invalid span".into())
}

#[test]
fn skipped_unknown_calls_do_not_escape_descriptors_or_become_predecessors() -> TestResult {
    for guard in [
        "false && missing(fd)",
        "true || missing(fd)",
        "(false) && (true || missing(fd))",
        "true && (false && missing(fd))",
    ] {
        let source = format!("{OPEN}{guard};{WRITE}");
        let report = forecast(&source)?;
        assert_eq!(array(&report["edits"])?.len(), 1, "{guard}");
        assert!(no_barriers(&report));
        let conditions = array(&prediction(&report, 0)["predecessorConditions"])?;
        assert!(
            conditions
                .iter()
                .any(|span| span_text(&source, span).is_ok_and(|s| s == guard))
        );
        assert!(
            !conditions
                .iter()
                .any(|span| span_text(&source, span).is_ok_and(|s| s == "missing(fd)"))
        );
    }
    Ok(())
}

#[test]
fn selected_unknown_calls_stop_the_following_write() -> TestResult {
    for guard in ["true && missing(fd)", "false || missing(fd)"] {
        let report = forecast(&format!("{OPEN}{guard};{WRITE}"))?;
        assert_eq!(report["disposition"], "analyzed");
        assert!(array(&report["edits"])?.is_empty());
        assert_eq!(report["javascriptBarriers"][0]["reason"], "unmodeledCall");
    }
    Ok(())
}

#[test]
fn discarded_selected_writes_execute_once_and_advance_utf8_cursor() -> TestResult {
    let source =
        format!("{OPEN}true && fs.writeSync(fd,'β'); false || fs.writeSync(fd,'Z'); {WRITE}");
    let report = forecast(&source)?;
    assert_eq!(array(&report["edits"])?.len(), 3);
    for (ordinal, offset) in [(0, 0), (1, 2), (2, 3)] {
        assert_eq!(
            prediction(&report, ordinal)["writeEffect"]["byteOffset"],
            offset
        );
        assert_eq!(prediction(&report, ordinal)["ordinal"], ordinal);
    }
    let first = prediction(&report, 0);
    assert!(!array(&first["predecessorConditions"])?.iter().any(|span| {
        span_text(&source, span).is_ok_and(|s| s == "true && fs.writeSync(fd,'β')")
    }));
    assert!(
        array(&prediction(&report, 1)["predecessorConditions"])?
            .iter()
            .any(|span| {
                span_text(&source, span).is_ok_and(|s| s == "true && fs.writeSync(fd,'β')")
            })
    );
    Ok(())
}

#[test]
fn selector_bindings_are_preserved_on_selected_and_subsequent_effects() -> TestResult {
    let report = forecast(&format!(
        "{OPEN}let flag=true; flag && {WRITE} flag=false; flag || {WRITE} {WRITE}"
    ))?;
    assert_eq!(array(&report["edits"])?.len(), 3);
    for (ordinal, versions) in [(0, vec![3]), (1, vec![3, 4]), (2, vec![3, 4])] {
        let actual: Vec<_> = array(&prediction(&report, ordinal)["dependencies"])?
            .iter()
            .filter(|d| d["type"] == "binding" && d["name"] == "flag")
            .map(|d| d["version"].as_u64().ok_or("missing version"))
            .collect::<Result<_, _>>()?;
        assert_eq!(actual, versions);
    }
    Ok(())
}

#[test]
fn nested_boolean_selector_chooses_one_write() -> TestResult {
    let report = forecast(&format!("{OPEN}(true && false) || {WRITE}"))?;
    assert_eq!(array(&report["edits"])?.len(), 1);
    assert!(no_barriers(&report));
    Ok(())
}

#[test]
fn consumed_inner_write_is_refused_before_any_effect() -> TestResult {
    let report = forecast(&format!(
        "{OPEN}(true && fs.writeSync(fd,'bad')) || {WRITE}"
    ))?;
    assert!(array(&report["edits"])?.is_empty());
    assert_eq!(array(&report["javascriptBarriers"])?.len(), 1);
    Ok(())
}

#[test]
fn selected_non_boolean_nested_result_stops_before_following_write() -> TestResult {
    let report = forecast(&format!("{OPEN}(true && 'text') || {WRITE}"))?;
    assert!(array(&report["edits"])?.is_empty());
    assert_eq!(
        report["javascriptBarriers"][0]["reason"],
        "unsupportedOperation"
    );
    Ok(())
}

#[test]
fn uninitialized_boolean_binding_is_not_assumed_false() -> TestResult {
    let report = forecast(&format!(
        "{OPEN}flag && missing(); const flag=false; {WRITE}"
    ))?;
    assert!(array(&report["edits"])?.is_empty());
    assert_eq!(report["javascriptBarriers"][0]["reason"], "unknownName");
    Ok(())
}

#[test]
fn skipped_writes_and_close_leave_the_descriptor_cursor_untouched() -> TestResult {
    let report = forecast(&format!(
        "{OPEN}false && fs.writeSync(fd,'bad'); true || fs.closeSync(fd); {WRITE}"
    ))?;
    assert_eq!(array(&report["edits"])?.len(), 1);
    assert_eq!(prediction(&report, 0)["writeEffect"]["byteOffset"], 0);
    Ok(())
}

#[test]
fn selected_close_invalidates_the_owned_descriptor() -> TestResult {
    let report = forecast(&format!("{OPEN}true && fs.closeSync(fd); {WRITE}"))?;
    assert!(array(&report["edits"])?.is_empty());
    assert_eq!(array(&report["javascriptBarriers"])?.len(), 1);
    Ok(())
}

#[test]
fn skipped_reads_do_not_allocate_read_identity_or_dependencies() -> TestResult {
    let source = "const fs=require('fs'); false && fs.readFileSync('skipped','utf8'); const text=fs.readFileSync('input','utf8'); fs.writeFileSync('out',text);";
    let report = forecast(source)?;
    let edit = prediction(&report, 0);
    assert_eq!(edit["content"]["readId"], 0);
    assert_eq!(edit["content"]["target"]["path"], "input");
    assert_eq!(
        array(&edit["dependencies"])?
            .iter()
            .filter(|d| d["type"] == "readValue")
            .count(),
        1
    );
    Ok(())
}

#[test]
fn non_boolean_selectors_do_not_use_javascript_truthiness() -> TestResult {
    for condition in ["0", "''", "'false'", "null", "fs", "fd", "unknown"] {
        let report = forecast(&format!("{OPEN}{condition} && missing();{WRITE}"))?;
        assert!(array(&report["edits"])?.is_empty(), "{condition}");
        assert_eq!(array(&report["javascriptBarriers"])?.len(), 1);
    }
    Ok(())
}

#[test]
fn lazy_values_cannot_initialize_descriptors_paths_or_payloads() -> TestResult {
    for expression in [
        "const saved=true && 'text';",
        "const second=true && fs.openSync('other','w');",
        "fs.writeFileSync(true && 'out','x');",
        "fs.writeSync(fd,true && 'x');",
        "true + 'x';",
        "fs.writeSync(fd,true);",
    ] {
        let report = forecast(&format!("{OPEN}{expression}{WRITE}"))?;
        assert!(array(&report["edits"])?.is_empty(), "{expression}");
    }
    Ok(())
}

#[test]
fn skipped_unsupported_or_malformed_syntax_discards_all_selective_edits() -> TestResult {
    for expression in [
        "false && (() => missing())();",
        "false && (flag=false);",
        "true || missing?.();",
        "false && (null ?? missing());",
        "false && missing(;",
    ] {
        let report = forecast(&format!("{OPEN}{WRITE}{expression}"))?;
        assert!(array(&report["edits"])?.is_empty(), "{expression}");
        assert_ne!(report["disposition"], "analyzed");
    }
    Ok(())
}

#[test]
fn live_unknown_suffix_retains_only_conditional_selective_prefix() -> TestResult {
    let report = forecast(&format!("{OPEN}{WRITE} true && missing(); {WRITE}"))?;
    assert_eq!(array(&report["edits"])?.len(), 1);
    assert_eq!(report["edits"][0]["laterContext"], "opaqueSuffix");
    Ok(())
}

#[test]
fn completion_budget_refuses_overlong_lazy_prefix_without_partial_edits() -> TestResult {
    let source = format!("{OPEN}{}{WRITE}", "false && missing();".repeat(257));
    let report = forecast(&source)?;
    assert!(array(&report["edits"])?.is_empty());
    assert_eq!(report["disposition"]["unavailable"], "sourceLimit");
    Ok(())
}

#[test]
fn conditional_forecasts_preserve_existing_whole_script_eligibility() -> TestResult {
    for (suffix, eligible) in [
        ("false && missing();", true),
        ("true && missing();", false),
        ("unknown && missing();", false),
        ("false && missing(;", false),
    ] {
        let request = Request::new(
            "whole-lazy".into(),
            Language::TypeScript,
            format!("const fs=require('fs'); fs.readFileSync('in','utf8'); {suffix}"),
            None,
        )
        .with_snapshots(vec![FileSnapshot::new(
            "in".into(),
            Some("old".into()),
            Some("new".into()),
        )]);
        let input = CompleteScript::from_request(
            request,
            EnvironmentAssumptions::FreshProcess,
            AnalysisLimits::new(32_768, Duration::from_secs(2)),
        )?;
        let activity = Analyzer::new()?.analyze_complete(input);
        if eligible {
            assert_eq!(activity.disposition(), PresentationDisposition::Eligible);
            assert_eq!(activity.ui_rows().len(), 1);
            assert_eq!(activity.diffs().len(), 1);
            continue;
        }
        assert!(matches!(
            activity.disposition(),
            PresentationDisposition::Fallback(_)
        ));
        assert!(activity.ui_rows().is_empty());
        assert!(activity.report().operations.is_empty());
        assert!(activity.diffs().is_empty());
        assert!(activity.report().diffs.is_empty());
    }
    Ok(())
}
