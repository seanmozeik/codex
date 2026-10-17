//! Authored inert sources: target-only suffix knowledge never executes Python.

use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::Language;
use codex_code_activity::SelectiveEditInput;
use pretty_assertions::assert_eq;
use serde_json::Value;
use std::time::Duration;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const PREFIX: &str =
    "with open('out','r+',encoding='utf8',newline='') as f:\n    f.write('X')\n    ";

fn forecast(source: &str, language: Language) -> TestResult<Value> {
    let input = SelectiveEditInput::from_source(
        "authored-suffix-touch".into(),
        language,
        source,
        None,
        AnalysisLimits::new(32_768, Duration::from_secs(2)),
    )?;
    Ok(serde_json::to_value(
        Analyzer::new()?.forecast_edits(input),
    )?)
}

fn python(body: &str) -> TestResult<Value> {
    forecast(&format!("{PREFIX}{body}"), Language::Python)
}

fn edits(report: &Value) -> TestResult<&Vec<Value>> {
    Ok(report["edits"].as_array().ok_or("missing edits")?)
}

fn assert_opaque_prefix(report: &Value, context: &str) -> TestResult {
    assert_eq!(report["disposition"], "analyzed");
    assert_eq!(edits(report)?.len(), 1);
    assert_eq!(report["edits"][0]["laterContext"], context);
    let prediction = &report["edits"][0]["details"]["prediction"];
    assert_eq!(prediction["finalState"], "opaqueSuccessor");
    assert_eq!(prediction["operation"]["type"], "logicalWrite");
    assert_eq!(prediction["operation"]["baseFilesystemEpoch"], 0);
    assert_eq!(prediction["operation"]["streamRevision"], 0);
    assert_eq!(
        prediction["operation"]["requiredResult"],
        "fullCharacterCount"
    );
    assert_eq!(
        report["pythonBarriers"]
            .as_array()
            .ok_or("missing barriers")?
            .len(),
        1
    );
    Ok(())
}

#[test]
fn same_owner_touch_changes_only_outer_context_not_prefix_or_barrier() -> TestResult {
    let base = python("f.flush()")?;
    let summarized = python("f.flush()\n    f.truncate(3)")?;
    assert_opaque_prefix(&base, "opaqueSuffix")?;
    assert_opaque_prefix(&summarized, "knownLaterTouch")?;
    assert_eq!(base["pythonBarriers"], summarized["pythonBarriers"]);
    let mut expected = base["edits"][0].clone();
    expected["laterContext"] = Value::String("knownLaterTouch".into());
    assert_eq!(expected, summarized["edits"][0]);
    assert!(summarized.get("suffixTouches").is_none());
    Ok(())
}

#[test]
fn literal_size_bounds_and_empty_truncate_are_not_evaluated() -> TestResult {
    for size in ["0", "3", "9223372036854775807"] {
        assert_opaque_prefix(
            &python(&format!("f.flush()\n    f.truncate({size})"))?,
            "knownLaterTouch",
        )?;
    }
    for size in ["", "-1", "1+2", "unknown()", "size", "size=3", "True"] {
        assert_opaque_prefix(
            &python(&format!("f.flush()\n    f.truncate({size})"))?,
            "opaqueSuffix",
        )?;
    }
    Ok(())
}

#[test]
fn unknown_operations_rebinding_aliases_and_other_receivers_block_summary() -> TestResult {
    for body in [
        "unknown()\n    f.flush()\n    f.truncate(3)",
        "alias=f\n    f.flush()\n    f.truncate(3)",
        "f=other\n    f.flush()\n    f.truncate(3)",
        "f.flush()\n    unknown()\n    f.truncate(3)",
        "f.flush()\n    f=other\n    f.truncate(3)",
        "f.flush()\n    other.truncate(3)",
        "f.flush()\n    f.close()\n    f.truncate(3)",
        "f.flush()\n    f.truncate(unknown())",
        "f.flush(unknown())\n    f.truncate(3)",
        "unknown.flush()\n    f.truncate(3)",
        "f.flush()\n    alias=f\n    alias.truncate(3)",
    ] {
        assert_opaque_prefix(&python(body)?, "opaqueSuffix")?;
    }
    Ok(())
}

#[test]
fn retained_touch_does_not_advance_into_later_unknown_or_known_io() -> TestResult {
    for tail in [
        "unknown()",
        "f.write('later')",
        "f.read()",
        "f.seek(3)",
        "f.close()",
    ] {
        let report = python(&format!("f.flush()\n    f.truncate(3)\n    {tail}"))?;
        assert_opaque_prefix(&report, "knownLaterTouch")?;
        assert_eq!(
            report["pythonBarriers"][0]["reason"],
            "unsupportedOperation"
        );
    }
    Ok(())
}

#[test]
fn suffix_summary_survives_owned_shell_unwind_without_leaking_to_other_edits() -> TestResult {
    let source = format!("open('prior','w').write('P')\n{PREFIX}f.flush()\n    f.truncate(3)");
    let shell = format!("python3 -c '{}'", source.replace('\'', "'\\''"));
    let direct = forecast(&source, Language::Python)?;
    let nested = forecast(&shell, Language::Shell)?;
    for report in [&direct, &nested] {
        assert_eq!(edits(report)?.len(), 3);
        assert_eq!(report["edits"][0]["laterContext"], "opaqueSuffix");
        assert_eq!(report["edits"][1]["laterContext"], "opaqueSuffix");
        assert_eq!(report["edits"][2]["laterContext"], "knownLaterTouch");
        assert_eq!(
            report["edits"][2]["details"]["prediction"]["finalState"],
            "opaqueSuccessor"
        );
        assert_eq!(
            report["pythonBarriers"]
                .as_array()
                .ok_or("missing barrier")?
                .len(),
            1
        );
    }
    assert_ne!(
        direct["edits"][2]["evidence"]["sourceId"],
        nested["edits"][2]["evidence"]["sourceId"]
    );
    Ok(())
}

#[test]
fn malformed_suffix_still_invalidates_the_whole_source() -> TestResult {
    for suffix in [
        "f.flush()\n    f.truncate(",
        "f.flush(*items)\n    f.truncate(3)",
        "f.flush()\n    f.truncate(*items)",
        "f.flush()\n    f.truncate(3.0)",
        "f.flush()\n    f.truncate(9223372036854775808)",
        "f.flush()\n    if unknown: f.truncate(3)",
        "f.flush()\n    raise Exception()",
    ] {
        let report = python(suffix)?;
        assert!(edits(&report)?.is_empty());
        assert_ne!(report["disposition"], "analyzed");
    }
    Ok(())
}
