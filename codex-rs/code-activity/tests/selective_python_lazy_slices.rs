//! Post-first-score development controls; all Python inputs remain inert text.

#[path = "selective_python_lazy_slices/support.rs"]
mod support;

use codex_code_activity::SnapshotPreviewError;
use pretty_assertions::assert_eq;
use serde_json::Value;
use support::Fixture;
use support::analyze;
use support::transform;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn view(source: &str) -> TestResult<Value> {
    Ok(serde_json::to_value(analyze(source)?)?)
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
fn lazy_unit_slices_use_codepoints_signed_bounds_and_python_clamping() -> TestResult {
    for (expression, expected) in [
        ("s[:-1] if s.endswith('\\n') else s", "é🧭ab"),
        ("s[2:] if s.startswith('é🧭') else s", "ab\n"),
        ("s[-4:-1] if s.startswith('é') else s", "🧭ab"),
        ("s[99:] if s.endswith('\\n') else s", ""),
        ("s[-99:99] if s.startswith('é') else s", "é🧭ab\n"),
        ("s[3:1] if s.startswith('é') else s", ""),
        ("s[:] if s.startswith('other') else s[:2]", "é🧭"),
        ("s[+1:] if s.startswith('é') else s", "🧭ab\n"),
    ] {
        assert_eq!(
            transform(expression, "é🧭ab\n".as_bytes())?
                .preview()
                .expect(expression)
                .after_bytes,
            expected.as_bytes()
        );
    }
    Ok(())
}

#[test]
fn bound_aliases_keep_values_and_argument_use_evidence_after_rebinding() -> TestResult {
    let source = "from pathlib import Path\nn=1\nsaved=n\nn=3\nend=-1\ns=Path('input').read_text(encoding='utf-8')\nPath('out').write_text(s[saved:end] if s.startswith('é') else s,encoding='utf-8')";
    let fixture = Fixture::python(source, Some("é🧭ab\n".as_bytes()))?;
    assert_eq!(
        fixture.preview().expect("captured bounds").after_bytes,
        "🧭ab".as_bytes()
    );
    let report = serde_json::to_value(&fixture.analysis).expect("serialize fixture analysis");
    let slice = &prediction(&report, 0)["operation"]["content"]["consequence"];
    assert_eq!(slice["start"]["value"], 1);
    assert_eq!(slice["end"]["value"], -1);
    assert_eq!(text(source, &slice["start"]["evidence"])?, "saved");
    Ok(())
}

#[test]
fn captured_index_method_freezes_receiver_and_literal_needle_alias() -> TestResult {
    let source = "from pathlib import Path\ns=Path('input').read_text(encoding='utf-8')\nbase=s\nseek=s.index\ns='rebound'\nneedle='🧭'\nx=base[seek(needle):] if base.startswith('é') else base\nneedle='later'\nPath('out').write_text(x,encoding='utf-8')";
    let fixture = Fixture::python(source, Some("é🧭ab\n".as_bytes()))?;
    assert_eq!(
        fixture.preview().expect("captured method").after_bytes,
        "🧭ab\n".as_bytes()
    );
    let report = serde_json::to_value(&fixture.analysis).expect("serialize fixture analysis");
    let slice = &prediction(&report, 0)["operation"]["content"]["consequence"];
    assert_eq!(slice["input"]["readId"], 0);
    assert_eq!(slice["start"]["input"]["readId"], 0);
    assert_eq!(slice["start"]["needle"], "🧭");
    assert_eq!(text(source, &slice["start"]["evidence"])?, "seek(needle)");
    Ok(())
}

#[test]
fn missing_literal_index_is_ignored_only_in_the_unselected_arm() -> TestResult {
    for expression in [
        "s[s.index('missing'):] if s.startswith('other') else 'safe'",
        "'safe' if s.startswith('plain') else s[s.index('missing'):]",
    ] {
        let source = format!(
            "from pathlib import Path\ns='plain'\nPath('out').write_text({expression},encoding='utf-8')"
        );
        assert_eq!(
            Fixture::python(&source, None)?
                .preview()
                .expect("unselected exception")
                .after_bytes,
            b"safe"
        );
    }
    for expression in [
        "s[s.index('missing'):] if s.startswith('plain') else 'safe'",
        "'safe' if s.startswith('other') else s[s.index('missing'):]",
    ] {
        let source = format!(
            "from pathlib import Path\ns='plain'\nPath('out').write_text({expression},encoding='utf-8')"
        );
        assert!(matches!(
            Fixture::python(&source, None)?.preview(),
            Err(SnapshotPreviewError::SubstringNotFound { .. })
        ));
    }
    Ok(())
}

#[test]
fn lazy_nested_index_starts_and_colon_needles_preserve_codepoint_semantics() -> TestResult {
    for (expression, expected) in [
        ("s[s.index('A',-2):] if s.startswith('é') else s", "A\n"),
        (
            "s[s.index('A',s.index('🧭')):] if s.startswith('é') else s",
            "A\n",
        ),
        ("s[s.index('',5):] if s.startswith('é') else s", ""),
    ] {
        assert_eq!(
            transform(expression, "éA🧭A\n".as_bytes())?
                .preview()
                .expect(expression)
                .after_bytes,
            expected.as_bytes()
        );
    }
    assert!(matches!(
        transform(
            "s[s.index('',6):] if s.startswith('é') else s",
            "éA🧭A\n".as_bytes()
        )?
        .preview(),
        Err(SnapshotPreviewError::SubstringNotFound { .. })
    ));
    assert_eq!(
        transform(
            "s[s.index(':'):] if s.startswith('é') else s",
            "é:a".as_bytes()
        )?
        .preview()
        .expect("colon in needle")
        .after_bytes,
        b":a"
    );
    Ok(())
}

#[test]
fn previously_evaluated_index_alias_keeps_its_existing_completion_condition() -> TestResult {
    let source = "from pathlib import Path\ns=Path('input').read_text(encoding='utf-8')\ni=s.index('🧭')\nsaved=i\ni=0\nPath('out').write_text(s[saved:] if s.startswith('é') else s,encoding='utf-8')";
    let fixture = Fixture::python(source, Some("é🧭ab".as_bytes()))?;
    assert_eq!(
        fixture.preview().expect("captured index").after_bytes,
        "🧭ab".as_bytes()
    );
    let report = serde_json::to_value(&fixture.analysis).expect("serialize fixture analysis");
    assert!(
        prediction(&report, 0)["predecessorConditions"]
            .as_array()
            .expect("predecessorConditions array")
            .iter()
            .map(|span| text(source, span))
            .collect::<TestResult<Vec<_>>>()?
            .contains(&"s.index('🧭')")
    );
    Ok(())
}

#[test]
fn discarded_or_overwritten_conditional_retains_only_its_whole_expression_guard() -> TestResult {
    let expression = "s[s.index('absent'):] if s.startswith('other') else s[s.index('a'):]";
    for statement in [expression.to_owned(), format!("x={expression}\nx='unused'")] {
        let source = format!(
            "from pathlib import Path\ns='abc'\n{statement}\nPath('out').write_text('literal',encoding='utf-8')"
        );
        let fixture = Fixture::python(&source, None)?;
        let preview = fixture.preview().expect("conditional operation point");
        assert_eq!(preview.after_bytes, b"literal");
        let preview = serde_json::to_value(preview).expect("serialize snapshot preview");
        assert_eq!(preview["basis"], "suppliedSnapshotConditional");
        assert_eq!(preview["scope"], "operationPoint");
        let conditions = preview["predecessorConditions"]
            .as_array()
            .expect("predecessorConditions array");
        assert_eq!(
            conditions
                .iter()
                .map(|span| text(&source, span))
                .collect::<TestResult<Vec<_>>>()?
                .iter()
                .filter(|text| **text == expression)
                .count(),
            1
        );
        for call in ["s.index('absent')", "s.index('a')"] {
            assert!(
                !conditions
                    .iter()
                    .map(|span| text(&source, span))
                    .collect::<TestResult<Vec<_>>>()?
                    .contains(&call)
            );
        }
    }
    Ok(())
}

#[test]
fn repeated_loop_conditionals_keep_distinct_completion_occurrences() -> TestResult {
    let expression = "s[s.index('absent'):] if s.startswith('other') else s[:]";
    let source = format!(
        "from pathlib import Path\ns='abc'\nfor name in ['one','two']:\n    {expression}\n    Path(name).write_text('ok',encoding='utf-8')"
    );
    let report = view(&source)?;
    assert_eq!(report["edits"].as_array().expect("edits array").len(), 2);
    for ordinal in 0..2 {
        let output = prediction(&report, ordinal);
        assert_eq!(output["iterationContext"][0]["index"], ordinal);
        assert_eq!(
            output["predecessorConditions"]
                .as_array()
                .expect("predecessorConditions array")
                .iter()
                .map(|span| text(&source, span))
                .collect::<TestResult<Vec<_>>>()?
                .iter()
                .filter(|text| **text == expression)
                .count(),
            ordinal + 1
        );
    }
    Ok(())
}

#[test]
fn effectful_unknown_arithmetic_and_wider_index_bounds_remain_barriers() -> TestResult {
    for arm in [
        "s[Path('hidden').write_text('x'):]",
        "open('hidden').read()[1:]",
        "s[s.index(Path('hidden').read_text()):]",
        "s[unknown():]",
        "s[s.index('a')+1:]",
        "s[0]",
        "s[s.find('a'):]",
        "s[s.index('a',0,1):]",
        "s[s.index(sub='a'):]",
    ] {
        let source = format!(
            "from pathlib import Path\nopen('prefix','w')\ns='a'\nx={arm} if s.startswith('other') else s\nopen('later','w')"
        );
        let report = view(&source)?;
        assert_eq!(report["disposition"], "analyzed");
        assert_eq!(report["edits"].as_array().expect("edits array").len(), 1);
        assert_eq!(prediction(&report, 0)["target"]["path"], "prefix");
        assert_eq!(prediction(&report, 0)["finalState"], "opaqueSuccessor");
    }
    Ok(())
}

#[test]
fn unsupported_unary_operators_cannot_form_lazy_bounds_or_counts() -> TestResult {
    for arm in ["s[~1:]", "s[s.index('a',~1):]", "s.replace('a','b',~1)"] {
        let report = view(&format!(
            "open('prefix','w')\ns='abc'\nx={arm} if s.startswith('other') else s"
        ))?;
        assert_ne!(report["disposition"], "analyzed");
        assert!(report["edits"].as_array().expect("edits array").is_empty());
    }
    Ok(())
}

#[test]
fn unsupported_binary_operators_cannot_form_lazy_text() -> TestResult {
    for arm in ["s*2", "s-'a'"] {
        let report = view(&format!(
            "open('prefix','w')\ns='abc'\nx={arm} if s.startswith('other') else s"
        ))?;
        assert_ne!(report["disposition"], "analyzed");
        assert!(report["edits"].as_array().expect("edits array").is_empty());
    }
    Ok(())
}

#[test]
fn unknown_integer_and_rebound_method_names_are_not_coerced_to_bounds() -> TestResult {
    for setup in ["n=s.count('a')", "n=s.index\nn=s.count('a')"] {
        for bound in ["n", "n('a')"] {
            let report = view(&format!(
                "open('prefix','w')\ns='abc'\n{setup}\nx=s[{bound}:] if s.startswith('other') else s\nopen('later','w')"
            ))?;
            assert_eq!(report["edits"].as_array().expect("edits array").len(), 1);
            assert_eq!(
                report["pythonBarriers"]
                    .as_array()
                    .expect("pythonBarriers array")
                    .len(),
                1
            );
        }
    }
    Ok(())
}

#[test]
fn unsupported_steps_and_malformed_lazy_slices_still_reject_complete_source() -> TestResult {
    for arm in ["s[::2]", "s[::-1]", "s[1:2:]"] {
        let report = view(&format!(
            "open('prefix','w')\ns='abc'\nx={arm} if s.startswith('a') else s"
        ))?;
        assert_ne!(report["disposition"], "analyzed");
        assert!(report["edits"].as_array().expect("edits array").is_empty());
    }
    let report = view("open('prefix','w')\ns='abc'\nx=s[1: if s.startswith('a') else s")?;
    assert_ne!(report["disposition"], "analyzed");
    assert!(report["edits"].as_array().expect("edits array").is_empty());
    Ok(())
}

#[test]
fn recursive_lazy_index_bounds_obey_recipe_and_construction_limits() -> TestResult {
    let bound = (0..70).fold("0".to_owned(), |start, _| format!("s.index('a',{start})"));
    let source = format!(
        "open('prefix','w')\ns='abc'\nx=s[{bound}:] if s.startswith('a') else s\nopen('later','w')"
    );
    let report = view(&source)?;
    assert_ne!(report["disposition"], "analyzed");
    assert!(report["edits"].as_array().expect("edits array").is_empty());
    Ok(())
}
