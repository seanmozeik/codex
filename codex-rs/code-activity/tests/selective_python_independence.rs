//! Independently authored source fixtures. No Python fixture is executed.

use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::Language;
use codex_code_activity::SelectiveEditInput;
use pretty_assertions::assert_eq;
use serde_json::Value;
use std::time::Duration;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn forecast(source: &str) -> TestResult<Value> {
    let input = SelectiveEditInput::from_source(
        "authored-python-independence".into(),
        Language::Python,
        source,
        None,
        AnalysisLimits::new(32_768, Duration::from_secs(2)),
    )?;
    let mut analyzer = Analyzer::new()?;
    Ok(serde_json::to_value(analyzer.forecast_edits(input))?)
}

fn prediction(report: &Value, ordinal: usize) -> &Value {
    &report["edits"][ordinal]["details"]["prediction"]
}

fn text<'a>(source: &'a str, span: &Value) -> TestResult<&'a str> {
    let start = usize::try_from(
        span["startByte"]
            .as_u64()
            .ok_or("missing evidence startByte")?,
    )?;
    let end = usize::try_from(span["endByte"].as_u64().ok_or("missing evidence endByte")?)?;
    Ok(source
        .get(start..end)
        .ok_or("evidence span is not a source substring")?)
}

fn complete(report: &Value, edits: usize) -> TestResult {
    assert_eq!(report["disposition"], "analyzed");
    assert!(
        report["pythonBarriers"]
            .as_array()
            .ok_or("expected pythonBarriers array")?
            .is_empty()
    );
    assert_eq!(
        report["edits"]
            .as_array()
            .ok_or("expected edits array")?
            .len(),
        edits
    );
    Ok(())
}

#[test]
fn discarded_standard_scalar_results_preserve_independent_writes_with_completion_evidence()
-> TestResult {
    for method in ["count", "find", "startswith", "endswith"] {
        let source = format!(
            "from pathlib import Path\ns = Path('source').read_text()\nn = s.{method}('missing')\nPath('out').write_text(s.replace('old', 'new'))"
        );
        let report = forecast(&source)?;
        complete(&report, 1)?;
        let output = prediction(&report, 0);
        assert_eq!(output["operation"]["content"]["type"], "replace");
        assert!(
            output["predecessorConditions"]
                .as_array()
                .expect("predecessorConditions array")
                .iter()
                .map(|span| text(&source, span))
                .collect::<TestResult<Vec<_>>>()?
                .contains(&format!("s.{method}('missing')").as_str())
        );
    }
    Ok(())
}

#[test]
fn missing_find_and_empty_count_need_no_substring_found_condition() -> TestResult {
    for call in ["'abc'.find('missing')", "'abc'.count('')"] {
        let report = forecast(&format!("{call}\nopen('out', 'w').write('done')"))?;
        complete(&report, 2)?;
        assert_eq!(
            prediction(&report, 1)["operation"]["content"]["value"],
            "done"
        );
    }
    Ok(())
}

#[test]
fn unknown_integer_rebinding_never_resurrects_initial_capabilities() -> TestResult {
    for source in [
        "s='abc'\nopen=s.count('a')\nopen('bad','w')",
        "from pathlib import Path\ns='abc'\nPath=s.find('a')\nPath('bad').write_text('x')",
        "from pathlib import Path\ns='abc'\nn=s.count('a')\nPath('bad').write_text(s.replace('a',n))",
    ] {
        let report = forecast(source)?;
        assert_eq!(report["disposition"], "analyzed");
        assert!(report["edits"].as_array().expect("edits array").is_empty());
        assert_eq!(
            report["pythonBarriers"]
                .as_array()
                .expect("pythonBarriers array")
                .len(),
            1
        );
    }
    Ok(())
}

#[test]
fn saved_capability_and_text_method_aliases_survive_scalar_rebinding() -> TestResult {
    let source = "writer=open\ns='abc'\nmeasure=s.count\ns='other'\nopen=measure('a')\nwriter('good','w').write('done')";
    let report = forecast(source)?;
    complete(&report, 2)?;
    assert_eq!(
        prediction(&report, 1)["operation"]["content"]["value"],
        "done"
    );
    assert!(
        prediction(&report, 0)["predecessorConditions"]
            .as_array()
            .expect("predecessorConditions array")
            .iter()
            .map(|span| text(source, span))
            .collect::<TestResult<Vec<_>>>()?
            .contains(&"measure('a')")
    );
    Ok(())
}

#[test]
fn modeled_argument_effect_precedes_scalar_signature_failure() -> TestResult {
    let source = "from pathlib import Path\ns='abc'\ns.count(Path('argument').write_text('seen'))\nPath('later').write_text('no')";
    let report = forecast(source)?;
    assert_eq!(report["disposition"], "analyzed");
    assert_eq!(report["edits"].as_array().expect("edits array").len(), 1);
    assert_eq!(prediction(&report, 0)["target"]["path"], "argument");
    assert_eq!(
        prediction(&report, 0)["operation"]["content"]["value"],
        "seen"
    );
    assert_eq!(prediction(&report, 0)["finalState"], "opaqueSuccessor");
    Ok(())
}

#[test]
fn arbitrary_calls_remain_barriers_even_for_literal_absolute_output() -> TestResult {
    for call in ["unknown()", "s.unknown()", "s.count(unknown())"] {
        let source =
            format!("open('prefix','w')\ns='abc'\n{call}\nopen('/absolute/out','w').write('no')");
        let report = forecast(&source)?;
        assert_eq!(report["disposition"], "analyzed");
        assert_eq!(report["edits"].as_array().expect("edits array").len(), 1);
        assert_eq!(prediction(&report, 0)["finalState"], "opaqueSuccessor");
    }
    Ok(())
}

#[test]
fn lazy_newline_conditional_retains_both_closed_recipes_and_one_read_identity() -> TestResult {
    let source = "from pathlib import Path\ns=Path('source').read_text()\ns=s if s.endswith('\\n') else s+'\\n'\nPath('out').write_text(s)";
    let report = forecast(source)?;
    complete(&report, 1)?;
    let output = prediction(&report, 0);
    let recipe = &output["operation"]["content"];
    assert_eq!(recipe["type"], "conditional");
    assert_eq!(recipe["predicate"]["type"], "endsWith");
    assert_eq!(recipe["predicate"]["suffix"], "\n");
    assert_eq!(recipe["predicate"]["input"], recipe["consequence"]);
    assert_eq!(recipe["alternative"]["left"], recipe["consequence"]);
    assert_eq!(recipe["consequence"]["readId"], 0);
    assert_eq!(recipe["alternative"]["right"]["value"], "\n");
    assert_eq!(
        text(source, &recipe["evidence"])?,
        "s if s.endswith('\\n') else s+'\\n'"
    );
    assert!(
        output["predecessorConditions"]
            .as_array()
            .expect("predecessorConditions array")
            .iter()
            .map(|span| text(source, span))
            .collect::<TestResult<Vec<_>>>()?
            .contains(&"s.endswith('\\n')")
    );
    Ok(())
}

#[test]
fn stored_predicate_freezes_receiver_and_argument_use_evidence_before_rebinding() -> TestResult {
    let source = "from pathlib import Path\nsuffix='\\n'\ns=Path('source').read_text()\ncheck=s.endswith\ns='rebound'\nflag=check(suffix)\nsuffix='new'\nPath('out').write_text('yes' if flag else s)";
    let report = forecast(source)?;
    complete(&report, 1)?;
    let recipe = &prediction(&report, 0)["operation"]["content"];
    assert_eq!(recipe["predicate"]["input"]["readId"], 0);
    assert_eq!(recipe["predicate"]["suffix"], "\n");
    assert_eq!(
        text(source, &recipe["predicate"]["suffixEvidence"])?,
        "suffix"
    );
    assert_eq!(
        text(source, &recipe["predicate"]["evidence"])?,
        "check(suffix)"
    );
    assert_eq!(recipe["alternative"]["value"], "rebound");
    Ok(())
}

#[test]
fn startswith_closed_replace_and_immutable_iadd_capture_prior_text() -> TestResult {
    let source = "from pathlib import Path\ns=Path('source').read_text()\nalias=s\ns+=('!' if s.startswith('#') else s.replace('old','new',1))\nPath('out').write_text(s)\nPath('alias').write_text(alias)";
    let report = forecast(source)?;
    complete(&report, 2)?;
    let recipe = &prediction(&report, 0)["operation"]["content"];
    assert_eq!(recipe["type"], "concat");
    assert_eq!(recipe["right"]["predicate"]["type"], "startsWith");
    assert_eq!(recipe["right"]["alternative"]["type"], "replaceText");
    assert_eq!(recipe["right"]["alternative"]["count"], 1);
    assert_eq!(
        recipe["left"],
        prediction(&report, 1)["operation"]["content"]
    );
    assert_eq!(recipe["left"], recipe["right"]["alternative"]["input"]);
    Ok(())
}

#[test]
fn valid_unsupported_conditional_arm_keeps_prefix_without_speculative_effects() -> TestResult {
    for arm in [
        "open('hidden','w')",
        "open('hidden').read()",
        "mystery()",
        "s[mystery():]",
        "s if s.endswith('y') else 'nested'",
    ] {
        let source = format!(
            "open('prefix','w')\ns='abc'\nx=s if s.endswith('x') else {arm}\nopen('later','w')"
        );
        let report = forecast(&source)?;
        assert_eq!(report["disposition"], "analyzed");
        assert_eq!(report["edits"].as_array().expect("edits array").len(), 1);
        assert_eq!(prediction(&report, 0)["target"]["path"], "prefix");
        assert_eq!(prediction(&report, 0)["finalState"], "opaqueSuccessor");
        assert_eq!(
            report["pythonBarriers"]
                .as_array()
                .expect("pythonBarriers array")
                .len(),
            1
        );
    }
    Ok(())
}

#[test]
fn malformed_conditional_suffix_rejects_complete_source_and_all_prefix_edits() -> TestResult {
    for expression in [
        "s if s.endswith('x') else",
        "s if else s",
        "s if s.endswith('x') s",
    ] {
        let report = forecast(&format!("open('prefix','w')\ns='abc'\nx={expression}"))?;
        assert_ne!(report["disposition"], "analyzed");
        assert!(report["edits"].as_array().expect("edits array").is_empty());
    }
    Ok(())
}

#[test]
fn wider_signatures_and_unknown_predicate_types_remain_outside_slice() -> TestResult {
    for call in [
        "s.endswith(('a','b'))",
        "s.count('a',0)",
        "s.find(sub='a')",
        "s.endswith(s+'x')",
    ] {
        let report = forecast(&format!(
            "open('prefix','w')\ns='abc'\n{call}\nopen('later','w')"
        ))?;
        assert_eq!(report["disposition"], "analyzed");
        assert_eq!(report["edits"].as_array().expect("edits array").len(), 1);
        assert_eq!(
            report["pythonBarriers"]
                .as_array()
                .expect("pythonBarriers array")
                .len(),
            1
        );
    }
    let report = forecast("s='abc'\nn=s.count('a')\nx='yes' if n else 'no'\nopen('later','w')")?;
    assert!(report["edits"].as_array().expect("edits array").is_empty());
    assert_eq!(
        report["pythonBarriers"]
            .as_array()
            .expect("pythonBarriers array")
            .len(),
        1
    );
    Ok(())
}

#[test]
fn repeated_loop_summary_conditions_keep_distinct_execution_occurrences() -> TestResult {
    let source = "from pathlib import Path\ns='abc'\nfor name in ['one','two']:\n    s.count('a')\n    Path(name).write_text('x')";
    let report = forecast(source)?;
    complete(&report, 2)?;
    for ordinal in 0..2 {
        let conditions = prediction(&report, ordinal)["predecessorConditions"]
            .as_array()
            .expect("predecessorConditions array");
        assert_eq!(
            conditions
                .iter()
                .map(|span| text(source, span))
                .collect::<TestResult<Vec<_>>>()?
                .iter()
                .filter(|text| **text == "s.count('a')")
                .count(),
            ordinal + 1
        );
    }
    Ok(())
}

#[test]
fn repeated_conditional_growth_obeys_resource_bounds_without_a_retained_prefix() -> TestResult {
    let source = format!(
        "open('prefix','w')\ns='abc'\n{}open('out','w').write(s)",
        "s=s if s.endswith('x') else s+'x'\n".repeat(70)
    );
    let report = forecast(&source)?;
    assert_ne!(report["disposition"], "analyzed");
    assert!(report["edits"].as_array().expect("edits array").is_empty());
    Ok(())
}
