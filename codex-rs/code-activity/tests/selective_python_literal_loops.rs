//! Authored parser inputs only; these Python programs are never executed.

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
        "authored-literal-loops".into(),
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

fn evidence_text<'a>(source: &'a str, evidence: &Value) -> TestResult<&'a str> {
    let start = usize::try_from(
        evidence["startByte"]
            .as_u64()
            .ok_or("missing evidence startByte")?,
    )?;
    let end = usize::try_from(
        evidence["endByte"]
            .as_u64()
            .ok_or("missing evidence endByte")?,
    )?;
    Ok(source
        .get(start..end)
        .ok_or("evidence span is not a source substring")?)
}

fn analyzed(report: &Value, edits: usize) -> TestResult {
    assert_eq!(report["disposition"], json!("analyzed"), "{report}");
    assert_eq!(
        report["edits"]
            .as_array()
            .ok_or("expected edits array")?
            .len(),
        edits,
        "{report}"
    );
    assert!(
        report["pythonBarriers"]
            .as_array()
            .ok_or("expected pythonBarriers array")?
            .is_empty(),
        "{report}"
    );
    Ok(())
}

#[test]
fn scalar_literal_iterations_keep_exact_calls_and_order() -> TestResult {
    let source = "from pathlib import Path\nfor name in ['first', 'second']:\n    Path(name).write_text(name)\nPath('after').write_text(name)";
    let report = forecast(source)?;
    analyzed(&report, 3)?;
    for (index, name) in ["first", "second"].iter().enumerate() {
        let edit = prediction(&report, index);
        assert_eq!(edit["ordinal"], json!(index));
        assert_eq!(edit["target"]["path"], json!(name));
        assert_eq!(edit["operation"]["content"]["value"], json!(name));
        assert_eq!(
            evidence_text(source, &edit["evidence"])?,
            "Path(name).write_text(name)"
        );
        assert_eq!(edit["iterationContext"][0]["index"], json!(index));
        assert_eq!(edit["iterationContext"][0]["literalCount"], json!(2));
        assert!(
            evidence_text(source, &edit["iterationContext"][0]["evidence"])?
                .starts_with("for name in")
        );
        assert!(
            edit["dependencies"]
                .as_array()
                .expect("dependencies array")
                .iter()
                .any(|d| {
                    d["type"] == "binding"
                        && d["name"] == "name"
                        && d["version"] == json!(index + 2)
                })
        );
    }
    assert!(prediction(&report, 2).get("iterationContext").is_none());
    assert_eq!(
        prediction(&report, 2)["operation"]["content"]["value"],
        json!("second")
    );
    Ok(())
}

#[test]
fn tuple_rows_unpack_into_distinct_plain_names() -> TestResult {
    for target in ["name, text", "(name, text)", "[name, text]"] {
        let source = format!(
            "from pathlib import Path\nfor {target} in (('a', 'one'), ('b', 'two')):\n    Path(name).write_text(text)"
        );
        let report = forecast(&source)?;
        analyzed(&report, 2)?;
        assert_eq!(prediction(&report, 0)["target"]["path"], json!("a"));
        assert_eq!(prediction(&report, 1)["target"]["path"], json!("b"));
        assert_eq!(
            prediction(&report, 0)["operation"]["content"]["value"],
            json!("one")
        );
        assert_eq!(
            prediction(&report, 1)["operation"]["content"]["value"],
            json!("two")
        );
    }
    Ok(())
}

#[test]
fn nested_iterations_expose_outer_to_inner_context() -> TestResult {
    let report = forecast(
        "from pathlib import Path\nfor name in ['a', 'b']:\n    for text in ('one', 'two'):\n        Path(name).write_text(text)",
    )?;
    analyzed(&report, 4)?;
    for index in 0..4 {
        let context = &prediction(&report, index)["iterationContext"];
        assert_eq!(context.as_array().expect("iterationContext array").len(), 2);
        assert_eq!(context[0]["index"], json!(index / 2));
        assert_eq!(context[1]["index"], json!(index % 2));
    }
    Ok(())
}

#[test]
fn body_rebinding_does_not_change_literal_iteration_or_old_aliases() -> TestResult {
    let report = forecast(
        "from pathlib import Path\ns = 'seed'\nfor name in ['a', 'b']:\n    alias = s\n    s += name\n    Path(name).write_text(alias)\n    name = 'changed'\nPath('after').write_text(s)",
    )?;
    analyzed(&report, 3)?;
    let first = &prediction(&report, 0)["operation"]["content"];
    let second = &prediction(&report, 1)["operation"]["content"];
    let last = &prediction(&report, 2)["operation"]["content"];
    assert_eq!(first["value"], json!("seed"));
    assert_eq!(second["type"], json!("concat"));
    assert_eq!(second["left"], *first);
    assert_eq!(second["right"]["value"], json!("a"));
    assert_eq!(last["left"], *second);
    assert_eq!(last["right"]["value"], json!("b"));
    assert_eq!(prediction(&report, 1)["target"]["path"], json!("b"));
    Ok(())
}

#[test]
fn repeated_reads_keep_distinct_ids_and_operation_point_epochs() -> TestResult {
    let source = "from pathlib import Path\nfor name in ['a', 'b']:\n    snapshot = Path('input').read_text()\n    Path(name).write_text(snapshot)";
    let report = forecast(source)?;
    analyzed(&report, 2)?;
    for index in 0..2 {
        let recipe = &prediction(&report, index)["operation"]["content"];
        assert_eq!(recipe["type"], json!("read"));
        assert_eq!(recipe["readId"], json!(index));
        assert_eq!(recipe["filesystemEpoch"], json!(index));
        assert_eq!(
            evidence_text(source, &recipe["evidence"])?,
            "Path('input').read_text()"
        );
    }
    Ok(())
}

#[test]
fn first_barrier_stops_remaining_body_iterations_and_suffix() -> TestResult {
    let report = forecast(
        "for name in ['a', 'b']:\n    open(name, 'w')\n    mystery()\n    open('body-suffix', 'w')\nopen('suffix', 'w')",
    )?;
    assert_eq!(report["disposition"], json!("analyzed"));
    assert_eq!(report["edits"].as_array().expect("edits array").len(), 1);
    assert_eq!(
        report["pythonBarriers"]
            .as_array()
            .expect("pythonBarriers array")
            .len(),
        1
    );
    assert_eq!(prediction(&report, 0)["target"]["path"], json!("a"));
    assert_eq!(
        prediction(&report, 0)["finalState"],
        json!("opaqueSuccessor")
    );
    Ok(())
}

#[test]
fn empty_loop_preserves_binding_and_still_checks_body_shape() -> TestResult {
    let report = forecast("name = 'old'\nfor name in []:\n    mystery()\nopen(name, 'w')")?;
    analyzed(&report, 1)?;
    assert_eq!(prediction(&report, 0)["target"]["path"], json!("old"));
    for body in [
        "break",
        "continue",
        "return",
        "import os",
        "if True:\n        pass",
        "x = f'{value}'",
        "__debug__ = 'x'",
    ] {
        let report = forecast(&format!("open('prefix', 'w')\nfor name in []:\n    {body}"))?;
        assert_ne!(report["disposition"], json!("analyzed"), "{body}: {report}");
        assert!(report["edits"].as_array().expect("edits array").is_empty());
    }
    Ok(())
}

#[test]
fn unsupported_loop_shapes_reject_the_whole_source() -> TestResult {
    for loop_source in [
        "for name in names:\n    pass",
        "for name in make_names():\n    pass",
        "for name in [other]:\n    pass",
        "for name in ['a' + 'b']:\n    pass",
        "for name in [f'{x}']:\n    pass",
        "for name in [b'a']:\n    pass",
        "for name in ['a', 1]:\n    pass",
        "for name in [('a', 'b')]:\n    pass",
        "for a, b in [('a',)]:\n    pass",
        "for a, b in [('a', 'b', 'c')]:\n    pass",
        "for a, b in [['a', 'b']]:\n    pass",
        "for a, a in [('a', 'b')]:\n    pass",
        "for (a) in [('a',)]:\n    pass",
        "for a, *b in [('a', 'b')]:\n    pass",
        "for obj.name in ['a']:\n    pass",
        "for __debug__ in []:\n    pass",
        "for name in []:\n    pass\nelse:\n    pass",
        "async for name in []:\n    pass",
        "for name in []:\n    def f():\n        pass",
        "for name in []:\n    pass\n  open('bad-indent', 'w')",
    ] {
        let report = forecast(&format!("open('prefix', 'w')\n{loop_source}"))?;
        assert_ne!(
            report["disposition"],
            json!("analyzed"),
            "{loop_source}: {report}"
        );
        assert!(report["edits"].as_array().expect("edits array").is_empty());
    }
    Ok(())
}

#[test]
fn loop_limits_fail_closed_even_for_pass_only_bodies() -> TestResult {
    let many = vec!["'x'"; 65].join(", ");
    let nesting = "for a in []:\n    for b in []:\n        for c in []:\n            for d in []:\n                for e in []:\n                    pass";
    let product = format!(
        "for a in [{}]:\n    for b in [{}]:\n        pass",
        vec!["'x'"; 16].join(", "),
        vec!["'y'"; 16].join(", ")
    );
    for suffix in [
        format!("for a in [{many}]:\n    pass"),
        nesting.to_owned(),
        product,
    ] {
        let report = forecast(&format!("open('prefix', 'w')\n{suffix}"))?;
        assert_ne!(report["disposition"], json!("analyzed"), "{report}");
        assert!(report["edits"].as_array().expect("edits array").is_empty());
    }
    Ok(())
}

#[test]
fn off_loop_serialization_has_no_iteration_context() -> TestResult {
    let report = forecast("open('a', 'w').write('text')")?;
    analyzed(&report, 2)?;
    for index in 0..2 {
        assert!(prediction(&report, index).get("iterationContext").is_none());
    }
    Ok(())
}
