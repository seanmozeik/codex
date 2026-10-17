//! Authored Node sources are parsed only; no JavaScript runtime is invoked.

use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::SelectiveEditInput;
use pretty_assertions::assert_eq;
use serde_json::Value;
use std::time::Duration;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn forecast(source: &str) -> TestResult<Value> {
    let input = SelectiveEditInput::from_node_commonjs(
        "authored-descriptor".into(),
        source,
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
fn open_write_truncation_is_a_separate_predecessor_and_epoch() -> TestResult {
    let report = forecast(
        "const fs=require('fs'); const fd=fs.openSync('out','w'); fs.writeSync(fd,'é',0,'utf8'); fs.closeSync(fd);",
    )?;
    assert_eq!(report["disposition"], "analyzed");
    assert_eq!(report["edits"].as_array().expect("edits").len(), 2);
    let truncate = prediction(&report, 0);
    let write = prediction(&report, 1);
    assert_eq!(truncate["writeEffect"]["type"], "truncate");
    assert_eq!(truncate["content"]["value"], "");
    assert_eq!(write["writeEffect"]["type"], "descriptorWrite");
    assert_eq!(write["writeEffect"]["descriptorId"], 0);
    assert_eq!(write["writeEffect"]["byteOffset"], 0);
    assert_eq!(write["writeEffect"]["position"], "absolute");
    assert_eq!(write["writeEffect"]["openedAt"], truncate["evidence"]);
    assert!(
        write["predecessorConditions"]
            .as_array()
            .expect("conditions")
            .contains(&truncate["evidence"])
    );
    assert!(
        write["dependencies"]
            .as_array()
            .expect("dependencies")
            .iter()
            .any(|dependency| dependency["type"] == "filesystemEpoch" && dependency["epoch"] == 1)
    );
    Ok(())
}

#[test]
fn cursor_counts_utf8_bytes_and_absolute_writes_leave_it_unchanged() -> TestResult {
    let report = forecast(
        "const fs=require('node:fs'); const fd=fs.openSync('out','r+'); fs.writeSync(fd,'é'); fs.writeSync(fd,'Z',5,'utf8'); fs.writeSync(fd,'😀',null); fs.writeSync(fd,'!',null,'utf8');",
    )?;
    assert_eq!(report["edits"].as_array().expect("edits").len(), 4);
    for (ordinal, offset, position) in [
        (0, 0, "cursor"),
        (1, 5, "absolute"),
        (2, 2, "cursor"),
        (3, 6, "cursor"),
    ] {
        let effect = &prediction(&report, ordinal)["writeEffect"];
        assert_eq!(effect["byteOffset"], offset);
        assert_eq!(effect["position"], position);
    }
    let dependencies = prediction(&report, 3)["dependencies"]
        .as_array()
        .expect("dependencies");
    assert!(dependencies.iter().any(|dependency| dependency["type"] == "descriptor"
        && dependency["descriptorId"] == 0));
    assert!(
        dependencies
            .iter()
            .any(|dependency| dependency["type"] == "binding"
                && dependency["name"] == "fd"
                && dependency["version"] == 2)
    );
    Ok(())
}

#[test]
fn short_write_exactness_is_an_explicit_conditional_premise() -> TestResult {
    let report = forecast(
        "const fs=require('fs'); const fd=fs.openSync('out','r+'); fs.writeSync(fd,'x');",
    )?;
    assert!(
        prediction(&report, 0)["dependencies"]
            .as_array()
            .expect("dependencies")
            .iter()
            .any(|dependency| dependency["type"] == "capability"
                && dependency["assumption"] == "completeDescriptorWrites")
    );
    assert!(
        prediction(&report, 0)["dependencies"]
            .as_array()
            .expect("dependencies")
            .iter()
            .any(|dependency| dependency["type"] == "capability"
                && dependency["assumption"] == "regularDescriptorFiles")
    );
    let ordinary = forecast("require('fs').writeFileSync('out','x');")?;
    assert!(prediction(&ordinary, 0).get("writeEffect").is_none());
    assert!(
        !ordinary["edits"][0]["details"]["assumptions"]
            .as_array()
            .expect("assumptions")
            .iter()
            .any(|assumption| assumption == "completeDescriptorWrites")
    );
    let truncate =
        forecast("const fs=require('fs'); const fd=fs.openSync('out','w'); fs.closeSync(fd);")?;
    assert!(
        !truncate["edits"][0]["details"]["assumptions"]
            .as_array()
            .expect("assumptions")
            .iter()
            .any(|assumption| assumption == "completeDescriptorWrites")
    );
    Ok(())
}

#[test]
fn closing_and_reopening_never_reuses_source_descriptor_identity() -> TestResult {
    let report = forecast(
        "const fs=require('fs'); const first=fs.openSync('a','r+'); fs.closeSync(first); const second=fs.openSync('b','r+'); fs.writeSync(second,'X'); fs.writeSync(first,'wrong');",
    )?;
    assert_eq!(report["edits"].as_array().expect("edits").len(), 1);
    assert_eq!(prediction(&report, 0)["writeEffect"]["descriptorId"], 1);
    assert_eq!(prediction(&report, 0)["target"]["path"], "b");
    assert_eq!(
        report["javascriptBarriers"][0]["reason"],
        "unsupportedOperation"
    );
    Ok(())
}

#[test]
fn descriptor_target_and_open_dependencies_are_frozen_before_path_reassignment() -> TestResult {
    let report = forecast(
        "const fs=require('fs'); let path='original'; const fd=fs.openSync(path,'r+'); path='other'; fs.writeSync(fd,'X');",
    )?;
    let edit = prediction(&report, 0);
    assert_eq!(edit["target"]["path"], "original");
    let dependencies = edit["dependencies"].as_array().expect("dependencies");
    assert!(
        dependencies
            .iter()
            .any(|dependency| dependency["type"] == "binding"
                && dependency["name"] == "path"
                && dependency["version"] == 2)
    );
    assert!(
        !dependencies
            .iter()
            .any(|dependency| dependency["type"] == "binding"
                && dependency["name"] == "path"
                && dependency["version"] == 4)
    );
    Ok(())
}

#[test]
fn unknown_byte_length_blocks_only_later_cursor_writes() -> TestResult {
    let report = forecast(
        "const fs=require('fs'); const text=fs.readFileSync('input','utf8'); const fd=fs.openSync('out','r+'); fs.writeSync(fd,text); fs.writeSync(fd,'x',2); fs.writeSync(fd,'y'); fs.writeFileSync('after','z');",
    )?;
    assert_eq!(report["edits"].as_array().expect("edits").len(), 2);
    assert_eq!(prediction(&report, 0)["writeEffect"]["byteOffset"], 0);
    assert_eq!(prediction(&report, 1)["writeEffect"]["byteOffset"], 2);
    assert_eq!(
        report["javascriptBarriers"][0]["reason"],
        "unsupportedOperation"
    );
    assert_eq!(report["edits"][1]["laterContext"], "opaqueSuffix");
    Ok(())
}

#[test]
fn explicit_position_accepts_read_recipes_without_advancing_cursor() -> TestResult {
    let report = forecast(
        "const fs=require('fs'); const text=fs.readFileSync('input','utf8'); const fd=fs.openSync('out','w'); fs.writeSync(fd,text.replace('a','b'),0,'utf8'); fs.writeSync(fd,'Z'); fs.closeSync(fd);",
    )?;
    assert_eq!(report["edits"].as_array().expect("edits").len(), 3);
    assert_eq!(prediction(&report, 1)["content"]["type"], "replaceFirst");
    assert_eq!(
        prediction(&report, 1)["content"]["input"]["filesystemEpoch"],
        0
    );
    assert_eq!(prediction(&report, 2)["writeEffect"]["byteOffset"], 0);
    Ok(())
}

#[test]
fn descriptor_alias_rebinding_escape_and_closed_lifetimes_stop_successors() -> TestResult {
    let prefix = "const fs=require('fs'); const fd=fs.openSync('out','r+');";
    for body in [
        "let alias=fd; fs.writeSync(alias,'x');",
        "fd=0; fs.writeSync(fd,'x');",
        "unknown(fd); fs.writeSync(fd,'x');",
        "fs.closeSync(fd); fs.writeSync(fd,'x');",
        "fs.closeSync(fd); fs.closeSync(fd);",
        "fs.writeSync(1,'x');",
        "fs.writeFileSync(fd,'x');",
    ] {
        let report = forecast(&format!("{prefix}{body} fs.writeFileSync('after','y');"))?;
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
fn only_direct_const_owners_and_supported_flags_can_open_descriptors() -> TestResult {
    for body in [
        "let fd=fs.openSync('out','w'); fs.writeSync(fd,'x');",
        "fs.openSync('out','w');",
        "fs.writeSync(fs.openSync('out','w'),'x');",
        "const fd=fs.openSync('out','a'); fs.writeSync(fd,'x',0);",
        "const fd=fs.openSync('out','r'); fs.writeSync(fd,'x');",
        "const fd=fs.openSync('out','wx'); fs.writeSync(fd,'x');",
    ] {
        let report = forecast(&format!("const fs=require('fs'); {body}"))?;
        assert!(
            report["edits"].as_array().expect("edits").is_empty(),
            "{body}"
        );
    }
    Ok(())
}

#[test]
fn numbers_are_finite_decimal_offsets_without_coercion() -> TestResult {
    for position in [
        "1.5",
        "-1",
        "0x10",
        "01",
        "1e2",
        "1n",
        "9007199254740992",
        "'0'",
        "'utf8'",
    ] {
        let report = forecast(&format!(
            "const fs=require('fs'); const fd=fs.openSync('out','r+'); fs.writeSync(fd,'x',{position});"
        ))?;
        assert!(
            report["edits"].as_array().expect("edits").is_empty(),
            "{position}"
        );
    }
    let report = forecast(
        "const fs=require('fs'); const fd=fs.openSync('out','r+'); const offset=12; fs.writeSync(fd,'x',offset);",
    )?;
    assert_eq!(prediction(&report, 0)["writeEffect"]["byteOffset"], 12);
    Ok(())
}

#[test]
fn unsupported_encoding_and_return_value_use_do_not_publish_descriptor_writes() -> TestResult {
    for body in [
        "fs.writeSync(fd,'x',0,'base64');",
        "fs.writeSync(fd,'x',0,'utf16le');",
        "const count=fs.writeSync(fd,'x');",
        "fs.writeSync(fd,0);",
        "fs.ftruncateSync(fd,2); fs.writeSync(fd,'x');",
    ] {
        let report = forecast(&format!(
            "const fs=require('fs'); const fd=fs.openSync('out','r+'); {body}"
        ))?;
        assert!(
            report["edits"].as_array().expect("edits").is_empty(),
            "{body}"
        );
    }
    Ok(())
}

#[test]
fn unsupported_control_flow_refuses_the_whole_descriptor_source() -> TestResult {
    for source in [
        "const fs=require('fs'); const fd=fs.openSync('out','w'); if (flag) {fs.writeSync(fd,'x');}",
        "const fs=require('fs'); const fd=fs.openSync('out','w'); for (;;) {fs.writeSync(fd,'x');}",
        "const fs=require('fs'); const fd=fs.openSync('out','w'); try {fs.writeSync(fd,'x');} finally {fs.closeSync(fd);}",
    ] {
        let report = forecast(source)?;
        assert!(
            report["edits"].as_array().expect("edits").is_empty(),
            "{source}"
        );
    }
    Ok(())
}
