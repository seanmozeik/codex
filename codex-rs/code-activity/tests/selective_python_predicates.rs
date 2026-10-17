//! Authored development controls. Submitted Python remains inert source text.

#[path = "selective_python_lazy_slices/support.rs"]
mod support;

use codex_code_activity::SnapshotPreviewError;
use pretty_assertions::assert_eq;
use serde_json::Value;
use support::Fixture;
use support::analyze;
use support::transform;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn prediction(report: &Value) -> &Value {
    &report["edits"][0]["details"]["prediction"]
}

fn evidence_text<'a>(source: &'a str, span: &Value) -> TestResult<&'a str> {
    let start = usize::try_from(span["startByte"].as_u64().ok_or("missing start")?)?;
    let end = usize::try_from(span["endByte"].as_u64().ok_or("missing end")?)?;
    source
        .get(start..end)
        .ok_or_else(|| "invalid source span".into())
}

#[test]
fn text_comparisons_preserve_unicode_membership_equality_and_empty_needles() -> TestResult {
    for (expression, input, expected) in [
        ("'hit' if '🧭' in s else 'miss'", "é🧭", "hit"),
        ("'hit' if '🧭é' in s else 'miss'", "é🧭", "miss"),
        ("'hit' if '' in s else 'miss'", "", "hit"),
        ("'same' if s == 'é🧭' else 'different'", "é🧭", "same"),
        ("'same' if 'é🧭' == s else 'different'", "é🧭", "same"),
        (
            "'same' if s == 'é' else 'different'",
            "e\u{301}",
            "different",
        ),
        ("'same' if s == 'a\\n' else 'different'", "a\r\n", "same"),
    ] {
        assert_eq!(
            transform(expression, input.as_bytes())?
                .preview()?
                .after_bytes,
            expected.as_bytes(),
            "{expression}"
        );
    }
    Ok(())
}

#[test]
fn membership_combines_with_counted_replace_and_conjunction_with_unicode_slices() -> TestResult {
    for (expression, input, expected) in [
        (
            "s.replace('red','青',2) if 'red' in s else s",
            "red\r\nred\rred",
            "青\n青\nred",
        ),
        (
            "s[1:-1] if s.startswith('«') and s.endswith('»') else s",
            "«é🧭»",
            "é🧭",
        ),
        (
            "s[1:-1] if s.startswith('«') and s.endswith('»') else s",
            "«é🧭",
            "«é🧭",
        ),
    ] {
        assert_eq!(
            transform(expression, input.as_bytes())?
                .preview()?
                .after_bytes,
            expected.as_bytes()
        );
    }
    Ok(())
}

#[test]
fn captured_receivers_predicates_and_operand_aliases_survive_rebinding() -> TestResult {
    let source = concat!(
        "from pathlib import Path\n",
        "s=Path('input').read_text(encoding='utf-8')\nbase=s\n",
        "start=s.startswith\nend=s.endswith\nneedle='🧭'\nsaved_needle=needle\n",
        "s='rebound'\nneedle='missing'\ngate=start('«') and end('»')\n",
        "saved=gate\ngate='discarded'\nstart='rebound'\n",
        "Path('out').write_text(base[1:-1] if saved and saved_needle in base else base,encoding='utf-8')",
    );
    let fixture = Fixture::python(source, Some("«é🧭»".as_bytes()))?;
    assert_eq!(fixture.preview()?.after_bytes, "é🧭".as_bytes());
    let report = serde_json::to_value(&fixture.analysis)?;
    let output = prediction(&report);
    let predicate = &output["operation"]["content"]["predicate"];
    assert_eq!(predicate["type"], "and");
    assert_eq!(predicate["left"]["left"]["type"], "startsWith");
    assert_eq!(predicate["left"]["left"]["input"]["readId"], 0);
    assert_eq!(predicate["right"]["needle"]["value"], "🧭");
    assert_eq!(
        evidence_text(source, &predicate["left"]["left"]["evidence"])?,
        "start('«')"
    );
    let dependencies = output["dependencies"].as_array().ok_or("dependencies")?;
    for name in ["base", "start", "end", "saved", "saved_needle"] {
        assert!(
            dependencies
                .iter()
                .any(|value| { value["type"] == "binding" && value["name"] == name }),
            "missing captured binding {name}"
        );
    }
    let start = dependencies
        .iter()
        .find(|value| value["type"] == "binding" && value["name"] == "start")
        .ok_or("start binding")?;
    assert_eq!(
        evidence_text(source, &start["evidence"])?,
        "start=s.startswith"
    );
    Ok(())
}

#[test]
fn short_circuit_skips_only_closed_rhs_exceptions_and_evaluates_selected_rhs() -> TestResult {
    for rhs in [
        "s[s.index('absent'):] == ''",
        "s[s.index('absent'):].startswith('x')",
        "'x' in s[s.index('absent'):]",
    ] {
        for (left, selected) in [("s == 'other'", false), ("s == 'plain'", true)] {
            let source = format!(
                "from pathlib import Path\ns='plain'\nPath('out').write_text('yes' if {left} and ({rhs}) else 'no',encoding='utf-8')"
            );
            let result = Fixture::python(&source, None)?.preview();
            if selected {
                assert!(matches!(
                    result,
                    Err(SnapshotPreviewError::SubstringNotFound { .. })
                ));
            } else {
                assert_eq!(result?.after_bytes, b"no");
            }
        }
    }
    Ok(())
}

#[test]
fn predicate_evaluates_before_selected_text_arm_and_comparisons_evaluate_left_first() -> TestResult
{
    let fixture = transform(
        "s[s.index('absent'):] if s == 'other' and 'x' in s else 'safe'",
        b"plain",
    )?;
    assert_eq!(fixture.preview()?.after_bytes, b"safe");
    for operator in ["==", "in"] {
        let expression = format!("s[s.index('LEFT'):] {operator} s[s.index('RIGHT'):]");
        let source = format!(
            "from pathlib import Path\ns='plain'\nPath('out').write_text('yes' if {expression} else 'no',encoding='utf-8')"
        );
        let error = Fixture::python(&source, None)?
            .preview()
            .expect_err("left operand raises");
        let value = serde_json::to_value(error)?;
        assert_eq!(
            evidence_text(&source, &value["evidence"])?,
            "s.index('LEFT')"
        );
    }
    Ok(())
}

#[test]
fn discarded_conjunction_retains_whole_expression_guard_without_unconditional_rhs_call()
-> TestResult {
    let expression = "s == 'other' and s[s.index('absent'):] == ''";
    let source = format!(
        "from pathlib import Path\ns='plain'\nx={expression}\nx='unused'\nPath('out').write_text('safe',encoding='utf-8')"
    );
    let fixture = Fixture::python(&source, None)?;
    let preview = fixture.preview()?;
    assert_eq!(preview.after_bytes, b"safe");
    let report = serde_json::to_value(preview)?;
    let spans = report["predecessorConditions"]
        .as_array()
        .ok_or("conditions")?;
    let conditions = spans
        .iter()
        .map(|span| evidence_text(&source, span))
        .collect::<TestResult<Vec<_>>>()?;
    assert_eq!(
        conditions
            .iter()
            .filter(|value| **value == expression)
            .count(),
        1
    );
    assert!(!conditions.contains(&"s.index('absent')"));
    assert_eq!(report["basis"], "suppliedSnapshotConditional");
    assert_eq!(report["scope"], "operationPoint");
    Ok(())
}

#[test]
fn conditional_forecast_keeps_owned_close_evidence_and_opaque_suffix_barrier() -> TestResult {
    let source = concat!(
        "with open('file',encoding='utf-8') as h:\n    s=h.read()\n",
        "out=s.replace('draft','ready') if s == 'draft\\n' else s\n",
        "with open('file','w',encoding='utf-8') as h:\n    h.write(out)\n",
        "opaque_hook()\n",
    );
    let report = serde_json::to_value(analyze(source)?)?;
    assert_eq!(report["edits"].as_array().ok_or("edits")?.len(), 2);
    assert_eq!(
        report["pythonBarriers"].as_array().ok_or("barriers")?.len(),
        1
    );
    let write = &report["edits"][1]["details"]["prediction"];
    assert_eq!(
        evidence_text(source, &write["evidence"])?,
        "with open('file','w',encoding='utf-8') as h:\n    h.write(out)"
    );
    assert!(
        write["predecessorConditions"]
            .as_array()
            .ok_or("write conditions")?
            .iter()
            .any(|span| evidence_text(source, span).is_ok_and(|text| text == "h.write(out)"))
    );
    assert_eq!(write["finalState"], "opaqueSuccessor");
    Ok(())
}

#[test]
fn unselected_rhs_consumes_no_read_but_selected_rhs_requires_its_exact_identity() -> TestResult {
    for (left, selected) in [("s == 'other'", false), ("s == 'plain'", true)] {
        let source = format!(
            "from pathlib import Path\ns=Path('input').read_text(encoding='utf-8')\nother=Path('other').read_text(encoding='utf-8')\nPath('out').write_text('yes' if {left} and other == 'needed' else 'no',encoding='utf-8')"
        );
        let result = Fixture::python(&source, Some(b"plain"))?.preview();
        if selected {
            assert!(
                matches!(result, Err(SnapshotPreviewError::MissingRead { read }) if read.read_id == 1)
            );
        } else {
            let preview = result?;
            assert_eq!(preview.after_bytes, b"no");
            assert_eq!(preview.read_bindings.len(), 1);
            assert_eq!(preview.read_bindings[0].read.read_id, 0);
        }
    }
    Ok(())
}

#[test]
fn actual_cst_wrappers_comments_and_nested_conjunctions_keep_finite_shapes() -> TestResult {
    for expression in [
        "'yes' if (('🧭') in (s)) else 'no'",
        "'yes' if ((s) == ('é🧭')) else 'no'",
        "'yes' if ((s.startswith('é')) and ('🧭' in s)) and (s == 'é🧭') else 'no'",
        "'yes' if ('🧭' # left operand\n in s) else 'no'",
    ] {
        assert_eq!(
            transform(expression, "é🧭".as_bytes())?
                .preview()?
                .after_bytes,
            b"yes"
        );
    }
    Ok(())
}
