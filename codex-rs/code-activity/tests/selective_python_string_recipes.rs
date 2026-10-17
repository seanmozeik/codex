//! Authored, parse-only fixtures; these Python strings are never executed.

use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::Language;
use codex_code_activity::SelectiveEditInput;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::time::Duration;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn forecast(source: &str) -> TestResult<Value> {
    let input = SelectiveEditInput::from_source(
        "authored-string-recipes".into(),
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

#[test]
fn captured_index_slice_and_dynamic_replace_preserve_binding_and_read_identity() -> TestResult {
    let source = "from pathlib import Path\ns = Path('source').read_text()\ni = s.index('START')\nj = s.index('END', i)\nold = s[i:j]\nreplacement = old + 'suffix'\ns = s.replace(old, replacement)\nPath('output').write_text(s)";
    let report = forecast(source)?;
    assert_eq!(report["disposition"], "analyzed");
    assert!(
        report["pythonBarriers"]
            .as_array()
            .expect("pythonBarriers array")
            .is_empty()
    );
    assert_eq!(report["edits"].as_array().expect("edits array").len(), 1);
    let output = prediction(&report, 0);
    let recipe = &output["operation"]["content"];
    assert_eq!(recipe["type"], "replaceText");
    assert_eq!(recipe["input"]["readId"], 0);
    assert_eq!(recipe["old"]["type"], "slice");
    assert_eq!(recipe["old"]["start"]["type"], "substringIndex");
    assert_eq!(recipe["old"]["end"]["start"], recipe["old"]["start"]);
    assert_eq!(recipe["new"]["left"], recipe["old"]);
    assert_eq!(text(source, &recipe["old"]["evidence"])?, "s[i:j]");
    for call in ["s.index('START')", "s.index('END', i)"] {
        assert!(
            output["predecessorConditions"]
                .as_array()
                .expect("predecessorConditions array")
                .iter()
                .map(|span| text(source, span))
                .collect::<TestResult<Vec<_>>>()?
                .contains(&call)
        );
    }
    for name in ["s", "i", "j", "old", "replacement"] {
        assert!(
            output["dependencies"]
                .as_array()
                .expect("dependencies array")
                .iter()
                .any(|d| d["type"] == "binding" && d["name"] == name)
        );
    }
    Ok(())
}

#[test]
fn split_negative_index_rstrip_and_unicode_are_unevaluated_recipes() -> TestResult {
    let source = "from pathlib import Path\ns = Path('source').read_text()\nf = s.split\ns = 'rebound'\nx = f('é', 1)[-1].rstrip('\\n界')\nPath('output').write_text(x)";
    let report = forecast(source)?;
    assert_eq!(report["disposition"], "analyzed");
    assert!(
        report["pythonBarriers"]
            .as_array()
            .expect("pythonBarriers array")
            .is_empty()
    );
    let output = prediction(&report, 0);
    let recipe = &output["operation"]["content"];
    assert_eq!(recipe["type"], "rstrip");
    assert_eq!(recipe["characters"], "\n界");
    assert_eq!(recipe["input"]["type"], "splitItem");
    assert_eq!(recipe["input"]["separator"], "é");
    assert_eq!(recipe["input"]["maxsplit"], 1);
    assert_eq!(recipe["input"]["index"], -1);
    assert_eq!(recipe["input"]["input"]["readId"], 0);
    assert_eq!(
        text(source, &recipe["input"]["splitEvidence"])?,
        "f('é', 1)"
    );
    assert!(
        output["predecessorConditions"]
            .as_array()
            .expect("predecessorConditions array")
            .iter()
            .map(|span| text(source, span))
            .collect::<TestResult<Vec<_>>>()?
            .contains(&"f('é', 1)[-1]")
    );
    Ok(())
}

#[test]
fn omitted_and_signed_slice_bounds_keep_python_code_point_semantics() -> TestResult {
    for (expression, start, end) in [
        ("s[:]", None, None),
        ("s[:-1]", None, Some(-1)),
        ("s[+1:]", Some(1), None),
        ("s[-9:99]", Some(-9), Some(99)),
    ] {
        let report = forecast(&format!(
            "from pathlib import Path\ns = 'é界'\nPath('output').write_text({expression})"
        ))?;
        assert_eq!(report["disposition"], "analyzed");
        let recipe = &prediction(&report, 0)["operation"]["content"];
        assert_eq!(recipe["type"], "slice");
        assert_eq!(recipe["input"]["value"], "é界");
        for (name, bound) in [("start", start), ("end", end)] {
            match bound {
                Some(value) => assert_eq!(recipe[name]["value"], value),
                None => assert!(recipe[name].is_null()),
            }
        }
    }
    Ok(())
}

#[test]
fn literal_replace_keeps_original_wire_variant() -> TestResult {
    for arguments in ["'a', 'b'", "'a' + 'b', 'c' + 'd'"] {
        let report = forecast(&format!(
            "open('out', 'w').write('abc'.replace({arguments}))"
        ))?;
        assert_eq!(report["disposition"], "analyzed");
        assert_eq!(
            prediction(&report, 1)["operation"]["content"]["type"],
            "replace"
        );
    }
    Ok(())
}

#[test]
fn definite_errors_and_unknown_receivers_stop_before_later_writes() -> TestResult {
    for operation in [
        "'abc'.index('missing')",
        "'abc'.index('a', 1)",
        "'abc'.index('', 4)",
        "'éa'.index('é', 1)",
        "'abc'.split('')",
        "'abc'.split(',')[1]",
        "'abc'.split(',')[-2]",
        "'a,b'.split(',', 0)[1]",
        "(1).rstrip('x')",
        "'abc'.split(',')[mystery()]",
        "'abc'.split(',').append('x')",
    ] {
        let report = forecast(&format!(
            "open('prefix', 'w')\n{operation}\nopen('later', 'w')"
        ))?;
        assert_eq!(report["disposition"], "analyzed", "{operation}: {report}");
        assert_eq!(report["edits"].as_array().expect("edits array").len(), 1);
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
fn index_argument_io_precedes_type_failure_but_unknown_methods_do_not_harvest_arguments()
-> TestResult {
    let report = forecast("s = 'abc'\ns.index(open('argument', 'w'))\nopen('later', 'w')")?;
    assert_eq!(report["disposition"], "analyzed");
    assert_eq!(report["edits"].as_array().expect("edits array").len(), 1);
    assert_eq!(prediction(&report, 0)["operation"]["type"], "truncate");
    let report = forecast("s = 'abc'\ns.unknown(open('argument', 'w'))")?;
    assert_eq!(report["disposition"], "analyzed");
    assert!(report["edits"].as_array().expect("edits array").is_empty());
    Ok(())
}

#[test]
fn discarded_potentially_failing_index_still_guards_following_write() -> TestResult {
    let source = "s = open('source').read()\ns.index('needle')\ns.split(':')[99]\nopen('out', 'w')";
    let report = forecast(source)?;
    assert_eq!(report["disposition"], "analyzed");
    let conditions = prediction(&report, 0)["predecessorConditions"]
        .as_array()
        .expect("predecessorConditions array");
    for operation in ["s.index('needle')", "s.split(':')[99]"] {
        assert!(
            conditions
                .iter()
                .map(|span| text(source, span))
                .collect::<TestResult<Vec<_>>>()?
                .contains(&operation)
        );
    }
    Ok(())
}

#[test]
fn unsupported_slice_steps_reject_whole_module() -> TestResult {
    for expression in ["s[::2]", "s[::-1]", "s[1:2:]"] {
        let report = forecast(&format!("open('prefix', 'w')\ns = 'abc'\nx = {expression}"))?;
        assert_ne!(report["disposition"], "analyzed");
        assert!(report["edits"].as_array().expect("edits array").is_empty());
    }
    Ok(())
}

#[test]
fn captured_string_method_retains_pre_mutation_read_epoch() -> TestResult {
    let source = "s = open('source').read()\nf = s.rstrip\nopen('source', 'w')\ns = 'new'\nopen('out', 'w').write(f('x'))";
    let report = forecast(source)?;
    assert_eq!(report["disposition"], "analyzed");
    let output = prediction(&report, 2);
    assert_eq!(output["operation"]["content"]["input"]["readId"], 0);
    assert_eq!(
        output["operation"]["content"]["input"]["filesystemEpoch"],
        0
    );
    assert!(
        output["dependencies"]
            .as_array()
            .expect("dependencies array")
            .iter()
            .any(|d| d == &json!({"type":"filesystemEpoch","epoch":2}))
    );
    Ok(())
}

#[test]
fn nested_string_recipes_obey_existing_depth_bound() -> TestResult {
    let source = format!(
        "s = 'abc'\n{}open('out', 'w').write(s)",
        "s = s.rstrip('x')\n".repeat(70)
    );
    let report = forecast(&source)?;
    assert_ne!(report["disposition"], "analyzed");
    assert!(report["edits"].as_array().expect("edits array").is_empty());
    Ok(())
}

#[test]
fn slice_evaluates_later_bound_before_rejecting_earlier_bound_type() -> TestResult {
    let report = forecast("s = 'abc'\ns[[]:open('bound', 'w').write('x')]\nopen('later', 'w')")?;
    assert_eq!(report["disposition"], "analyzed");
    assert_eq!(report["edits"].as_array().expect("edits array").len(), 2);
    assert_eq!(prediction(&report, 0)["operation"]["type"], "truncate");
    assert_eq!(prediction(&report, 1)["operation"]["type"], "writeText");
    assert_eq!(
        report["pythonBarriers"]
            .as_array()
            .expect("pythonBarriers array")
            .len(),
        1
    );
    Ok(())
}
