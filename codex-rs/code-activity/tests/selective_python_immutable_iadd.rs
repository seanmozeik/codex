//! Authored parser inputs only. These tests never execute the Python programs.

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
        "authored-immutable-iadd".into(),
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

#[test]
fn self_append_rebinds_string_without_changing_its_alias() -> TestResult {
    let source = "from pathlib import Path\ns = 'seed'\nalias = s\ns += s\nPath('old').write_text(alias)\nPath('new').write_text(s)";
    let report = forecast(source)?;
    assert_eq!(report["disposition"], json!("analyzed"));
    assert!(
        report["pythonBarriers"]
            .as_array()
            .expect("pythonBarriers array")
            .is_empty()
    );
    assert_eq!(report["edits"].as_array().expect("edits array").len(), 2);
    let original = &prediction(&report, 0)["operation"]["content"];
    assert_eq!(original["type"], json!("literal"));
    assert_eq!(original["value"], json!("seed"));
    let updated = prediction(&report, 1);
    let recipe = &updated["operation"]["content"];
    assert_eq!(recipe["type"], json!("concat"));
    assert_eq!(recipe["left"], *original);
    assert_eq!(recipe["right"], *original);
    assert_eq!(evidence_text(source, &recipe["evidence"])?, "s += s");
    assert_eq!(updated["operation"]["encoding"], json!("runtimeDefault"));
    assert!(
        updated["dependencies"]
            .as_array()
            .expect("dependencies array")
            .iter()
            .any(|d| { d["type"] == "binding" && d["name"] == "s" && d["version"] == 4 })
    );
    assert!(
        prediction(&report, 0)["dependencies"]
            .as_array()
            .expect("dependencies array")
            .iter()
            .any(|d| d["type"] == "binding" && d["name"] == "alias" && d["version"] == 3)
    );
    Ok(())
}

#[test]
fn captured_reads_keep_their_epochs_across_rhs_io_and_rebinding() -> TestResult {
    let source = "s = open('before').read()\nalias = s\nopen('epoch', 'w')\ns += open('after').read()\nopen('old', 'w').write(alias)\nopen('joined', 'w').write(s)";
    let report = forecast(source)?;
    assert_eq!(report["disposition"], json!("analyzed"));
    assert!(
        report["pythonBarriers"]
            .as_array()
            .expect("pythonBarriers array")
            .is_empty()
    );
    assert_eq!(report["edits"].as_array().expect("edits array").len(), 5);
    let original = &prediction(&report, 2)["operation"]["content"];
    assert_eq!(original["type"], json!("read"));
    assert_eq!(original["readId"], json!(0));
    assert_eq!(original["filesystemEpoch"], json!(0));
    let recipe = &prediction(&report, 4)["operation"]["content"];
    assert_eq!(recipe["type"], json!("concat"));
    assert_eq!(recipe["left"], *original);
    assert_eq!(recipe["right"]["readId"], json!(1));
    assert_eq!(recipe["right"]["filesystemEpoch"], json!(1));
    assert_eq!(
        evidence_text(source, &recipe["right"]["evidence"])?,
        "open('after').read()"
    );
    assert!(
        prediction(&report, 4)["predecessorConditions"]
            .as_array()
            .expect("predecessorConditions array")
            .iter()
            .map(|span| evidence_text(source, span))
            .collect::<TestResult<Vec<_>>>()?
            .contains(&"open('after').read()")
    );
    Ok(())
}

#[test]
fn unresolved_or_nontext_lhs_does_not_evaluate_rhs() -> TestResult {
    for prefix in [
        "",
        "s = 1\n",
        "s = []\n",
        "from pathlib import Path\ns = Path('path')\n",
    ] {
        let report = forecast(&format!("{prefix}s += open('rhs', 'w')"))?;
        assert_eq!(report["disposition"], json!("analyzed"));
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
fn opaque_rhs_blocks_rebinding_and_later_writes() -> TestResult {
    let report =
        forecast("open('prefix', 'w')\ns = 'left'\ns += mystery()\nopen('later', 'w').write(s)")?;
    assert_eq!(report["disposition"], json!("analyzed"));
    assert_eq!(report["edits"].as_array().expect("edits array").len(), 1);
    assert_eq!(
        prediction(&report, 0)["operation"]["type"],
        json!("truncate")
    );
    assert_eq!(
        prediction(&report, 0)["finalState"],
        json!("opaqueSuccessor")
    );
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
fn rhs_truncation_precedes_failed_text_concatenation() -> TestResult {
    let report = forecast("s = 'left'\ns += open('rhs', 'w')\nopen('later', 'w')")?;
    assert_eq!(report["disposition"], json!("analyzed"));
    assert_eq!(report["edits"].as_array().expect("edits array").len(), 1);
    assert_eq!(
        prediction(&report, 0)["operation"]["type"],
        json!("truncate")
    );
    assert_eq!(
        prediction(&report, 0)["finalState"],
        json!("opaqueSuccessor")
    );
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
fn unsupported_targets_operators_and_late_syntax_reject_whole_module() -> TestResult {
    for suffix in [
        "s.value += 'x'",
        "s[0] += 'x'",
        "s *= 2",
        "s -= 'x'",
        "__debug__ += 'x'",
        "s += f'{value}'",
        "s += 'x'\nif True:\n pass",
        "s += 'x'\ndef broken(:",
    ] {
        let report = forecast(&format!("open('prefix', 'w')\ns = 'left'\n{suffix}"))?;
        assert_ne!(report["disposition"], json!("analyzed"), "{report}");
        assert!(report["edits"].as_array().expect("edits array").is_empty());
    }
    Ok(())
}

#[test]
fn growing_augmented_recipes_preserve_the_existing_resource_limit() -> TestResult {
    let source = format!(
        "open('prefix', 'w')\ns = 'seed'\n{}",
        "s += 'x'\n".repeat(70)
    );
    let report = forecast(&source)?;
    assert_ne!(report["disposition"], json!("analyzed"), "{report}");
    assert!(report["edits"].as_array().expect("edits array").is_empty());
    Ok(())
}
