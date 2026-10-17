//! Authored strings are analyzed only; no Node process or target file is opened.

use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::SelectiveEditInput;
use pretty_assertions::assert_eq;
use serde_json::Value;
use std::time::Duration;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn forecast(body: &str) -> TestResult<Value> {
    let source = format!("const fs=require('fs'); {body}");
    let input = SelectiveEditInput::from_node_commonjs(
        "authored-append".into(),
        &source,
        Some("/authored"),
        AnalysisLimits::new(32_768, Duration::from_secs(2)),
    )?;
    Ok(serde_json::to_value(
        Analyzer::new()?.forecast_edits(input),
    )?)
}

fn prediction(report: &Value, ordinal: usize) -> &Value {
    &report["edits"][ordinal]["details"]["prediction"]
}

#[test]
fn append_alias_chain_keeps_identity_and_earlier_binding_dependencies() -> TestResult {
    let report = forecast(
        "const fd=fs.openSync('out','a'); const alias=fd; const second=alias; fs.writeSync(second,'é'); fs.writeSync(fd,'尾',null,'utf8');",
    )?;
    assert_eq!(report["edits"].as_array().expect("edits").len(), 2);
    let first = prediction(&report, 0);
    let next = prediction(&report, 1);
    assert_eq!(first["writeEffect"]["type"], "descriptorAppend");
    assert_eq!(first["writeEffect"]["descriptorId"], 0);
    assert_eq!(first["writeEffect"], next["writeEffect"]);
    assert!(first["writeEffect"].get("byteOffset").is_none());
    assert!(first["writeEffect"].get("position").is_none());
    let dependencies = first["dependencies"].as_array().expect("dependencies");
    for (name, version) in [("fs", 1), ("fd", 2), ("alias", 3), ("second", 4)] {
        assert!(dependencies.iter().any(|item| item["type"] == "binding"
            && item["name"] == name
            && item["version"] == version));
    }
    for premise in ["completeDescriptorWrites", "regularDescriptorFiles"] {
        assert!(
            dependencies
                .iter()
                .any(|item| item["type"] == "capability" && item["assumption"] == premise)
        );
    }
    let conditions = first["predecessorConditions"]
        .as_array()
        .expect("conditions");
    assert!(conditions.contains(&first["writeEffect"]["openedAt"]));
    assert!(!conditions.contains(&first["evidence"]));
    assert!(
        next["predecessorConditions"]
            .as_array()
            .expect("conditions")
            .contains(&first["evidence"])
    );
    assert!(
        next["dependencies"]
            .as_array()
            .expect("dependencies")
            .iter()
            .any(|item| item["type"] == "filesystemEpoch" && item["epoch"] == 1)
    );
    Ok(())
}

#[test]
fn closing_an_alias_invalidates_owner_and_all_other_aliases() -> TestResult {
    for body in [
        "fs.closeSync(alias); fs.writeSync(fd,'bad');",
        "fs.closeSync(fd); fs.writeSync(alias,'bad');",
        "fs.closeSync(alias); const later=fd; fs.writeSync(later,'bad');",
        "fs.closeSync(alias); fs.closeSync(fd);",
    ] {
        let report = forecast(&format!(
            "const fd=fs.openSync('out','a'); const alias=fd; {body} fs.writeFileSync('later','bad');"
        ))?;
        assert_eq!(report["disposition"], "analyzed", "{body}");
        assert!(
            report["edits"].as_array().expect("edits").is_empty(),
            "{body}"
        );
        assert_eq!(
            report["javascriptBarriers"][0]["reason"],
            "unsupportedOperation"
        );
    }
    Ok(())
}

#[test]
fn reopened_append_has_a_fresh_source_identity() -> TestResult {
    let report = forecast(
        "const first=fs.openSync('out','a'); const old=first; fs.closeSync(first); const next=fs.openSync('out','a'); fs.writeSync(next,'new'); fs.writeSync(old,'bad');",
    )?;
    assert_eq!(report["edits"].as_array().expect("edits").len(), 1);
    assert_eq!(prediction(&report, 0)["writeEffect"]["descriptorId"], 1);
    assert_eq!(report["edits"][0]["laterContext"], "opaqueSuffix");
    Ok(())
}

#[test]
fn positioned_aliases_share_cursor_without_changing_overlay_semantics() -> TestResult {
    let report = forecast(
        "const fd=fs.openSync('out','r+'); const alias=fd; fs.writeSync(alias,'é'); fs.writeSync(fd,'X'); fs.writeSync(alias,'Y',0); fs.writeSync(fd,'Z');",
    )?;
    for (ordinal, offset) in [(0, 0), (1, 2), (2, 0), (3, 3)] {
        let effect = &prediction(&report, ordinal)["writeEffect"];
        assert_eq!(effect["type"], "descriptorWrite");
        assert_eq!(effect["byteOffset"], offset);
    }
    Ok(())
}

#[test]
fn read_backed_append_does_not_need_a_statically_known_byte_cursor() -> TestResult {
    let report = forecast(
        "const saved=fs.readFileSync('out','utf8'); const fd=fs.openSync('out','a'); fs.writeSync(fd,saved); const fresh=fs.readFileSync('out','utf8'); fs.writeSync(fd,fresh); fs.writeSync(fd,saved);",
    )?;
    assert_eq!(report["edits"].as_array().expect("edits").len(), 3);
    for (ordinal, read, epoch) in [(0, 0, 0), (1, 1, 1), (2, 0, 0)] {
        let prediction = prediction(&report, ordinal);
        assert_eq!(prediction["writeEffect"]["type"], "descriptorAppend");
        assert_eq!(prediction["content"]["readId"], read);
        assert_eq!(prediction["content"]["filesystemEpoch"], epoch);
    }
    Ok(())
}

#[test]
fn append_refuses_numeric_positions_mutable_aliases_and_escape() -> TestResult {
    for body in [
        "fs.writeSync(fd,'bad',0);",
        "fs.writeSync(fd,'bad',1);",
        "fs.writeSync(fd,'bad','utf8');",
        "fs.writeSync(fd,'bad',null,'utf16le');",
        "let alias=fd; fs.writeSync(alias,'bad');",
        "fd=0; fs.writeSync(fd,'bad');",
        "unknown(fd); fs.writeSync(fd,'bad');",
        "fs.writeFileSync(fd,'bad');",
        "const count=fs.writeSync(fd,'bad');",
        "const alias=true && fd; fs.writeSync(alias,'bad');",
    ] {
        let report = forecast(&format!(
            "const fd=fs.openSync('out','a'); {body} fs.writeFileSync('later','bad');"
        ))?;
        assert!(
            report["edits"].as_array().expect("edits").is_empty(),
            "{body}"
        );
        assert_eq!(
            report["javascriptBarriers"]
                .as_array()
                .expect("barriers")
                .len(),
            1,
            "{body}"
        );
    }
    Ok(())
}

#[test]
fn only_selected_append_flags_are_admitted() -> TestResult {
    for mode in ["a+", "ax", "as", "r", "unknown"] {
        let report = forecast(&format!(
            "const fd=fs.openSync('out','{mode}'); fs.writeSync(fd,'bad');"
        ))?;
        assert!(
            report["edits"].as_array().expect("edits").is_empty(),
            "{mode}"
        );
    }
    Ok(())
}

#[test]
fn captured_canonical_functions_and_path_bindings_keep_the_opened_target() -> TestResult {
    let report = forecast(
        "const open=fs.openSync; const write=fs.writeSync; const close=fs.closeSync; let path='original'; let flags='a'; const fd=open(path,flags); const alias=fd; path='other'; flags='w'; write(alias,'X'); close(fd);",
    )?;
    assert_eq!(report["edits"].as_array().expect("edits").len(), 1);
    let edit = prediction(&report, 0);
    assert_eq!(edit["writeEffect"]["type"], "descriptorAppend");
    assert_eq!(edit["target"]["path"], "original");
    let dependencies = edit["dependencies"].as_array().expect("dependencies");
    for (name, version) in [("path", 5), ("flags", 6)] {
        assert!(dependencies.iter().any(|item| item["type"] == "binding"
            && item["name"] == name
            && item["version"] == version));
    }
    assert!(
        !dependencies.iter().any(
            |item| item["type"] == "binding" && (item["version"] == 9 || item["version"] == 10)
        )
    );
    Ok(())
}
