//! Authored Node source fixtures; the captured JavaScript is never executed.

use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::SelectiveEditInput;
use pretty_assertions::assert_eq;
use serde_json::Value;
use std::time::Duration;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn forecast(source: &str) -> TestResult<Value> {
    let input = SelectiveEditInput::from_node_commonjs(
        "authored-node-extension".into(),
        source,
        Some("/workspace"),
        AnalysisLimits::new(32_768, Duration::from_secs(2)),
    )?;
    let mut analyzer = Analyzer::new()?;
    Ok(serde_json::to_value(analyzer.forecast_edits(input))?)
}

fn prediction(report: &Value, ordinal: usize) -> &Value {
    &report["edits"][ordinal]["details"]["prediction"]
}

fn span_text<'source>(source: &'source str, span: &Value) -> TestResult<&'source str> {
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
fn all_match_recipe_remains_distinct_from_first_match() -> TestResult {
    let source = "const fs=require('fs'); fs.writeFileSync('all','aaaa'.replaceAll('aa','b')); fs.writeFileSync('first','aaaa'.replace('aa','b'));";
    let report = forecast(source)?;
    assert_eq!(report["edits"].as_array().expect("edits").len(), 2);
    let all = &prediction(&report, 0)["content"];
    assert_eq!(all["type"], "replaceAll");
    assert_eq!(all["old"], "aa");
    assert_eq!(all["new"], "b");
    assert_eq!(
        span_text(source, &all["evidence"])?,
        "'aaaa'.replaceAll('aa','b')"
    );
    assert_eq!(prediction(&report, 1)["content"]["type"], "replaceFirst");
    Ok(())
}

#[test]
fn replacement_literal_aliases_capture_their_versions() -> TestResult {
    let report = forecast(
        "const fs=require('fs'); let needle='é'; let replacement='😀'; const saved='éé'.replaceAll(needle,replacement); needle='x'; replacement='y'; fs.writeFileSync('out',saved);",
    )?;
    let edit = prediction(&report, 0);
    assert_eq!(edit["content"]["old"], "é");
    assert_eq!(edit["content"]["new"], "😀");
    let dependencies = edit["dependencies"].as_array().expect("dependencies");
    for (name, version) in [("needle", 2), ("replacement", 3)] {
        assert!(dependencies.iter().any(|value| {
            value["type"] == "binding" && value["name"] == name && value["version"] == version
        }));
    }
    Ok(())
}

#[test]
fn direct_destructured_imports_keep_function_binding_read_and_epoch_provenance() -> TestResult {
    for (module, members, read_name, write_name) in [
        (
            "fs",
            "readFileSync, writeFileSync",
            "readFileSync",
            "writeFileSync",
        ),
        (
            "node:fs",
            "readFileSync: read, writeFileSync: write,",
            "read",
            "write",
        ),
    ] {
        let declaration = format!("const {{ {members} }}=require('{module}');");
        let source = format!(
            "{declaration} const saved={read_name}('in','utf8'); {write_name}('out',saved.replaceAll('x','y'));"
        );
        let report = forecast(&source)?;
        assert_eq!(report["edits"].as_array().expect("edits").len(), 1);
        let edit = prediction(&report, 0);
        assert_eq!(edit["content"]["type"], "replaceAll");
        let read = &edit["content"]["input"];
        assert_eq!(read["type"], "readUtf8");
        assert_eq!(read["readId"], 0);
        assert_eq!(read["filesystemEpoch"], 0);
        assert_eq!(read["target"]["cwd"], "/workspace");
        assert_eq!(
            span_text(&source, &read["evidence"])?,
            format!("{read_name}('in','utf8')")
        );
        let dependencies = edit["dependencies"].as_array().expect("dependencies");
        for name in [read_name, write_name] {
            let binding = dependencies
                .iter()
                .find(|value| value["type"] == "binding" && value["name"] == name)
                .expect("function binding");
            assert_eq!(span_text(&source, &binding["evidence"])?, declaration);
        }
        let conditions = edit["predecessorConditions"]
            .as_array()
            .expect("conditions");
        assert_eq!(conditions.len(), 2);
        assert_eq!(
            span_text(&source, &conditions[0])?,
            format!("require('{module}')")
        );
    }
    Ok(())
}

#[test]
fn captured_destructured_read_retains_epoch_across_a_write() -> TestResult {
    let report = forecast(
        "const {readFileSync:r,writeFileSync:w}=require('fs'); const saved=r('in','utf8'); w('in','changed'); w('out',saved.replaceAll('x','y'));",
    )?;
    let edit = prediction(&report, 1);
    assert_eq!(edit["ordinal"], 1);
    assert_eq!(edit["content"]["input"]["filesystemEpoch"], 0);
    let dependencies = edit["dependencies"].as_array().expect("dependencies");
    for epoch in [0, 1] {
        assert!(
            dependencies
                .iter()
                .any(|value| value["type"] == "filesystemEpoch" && value["epoch"] == epoch)
        );
    }
    Ok(())
}

#[test]
fn invalid_destructuring_or_shadowed_canonical_names_refuses_the_whole_source() -> TestResult {
    let prefix = "require('fs').writeFileSync('before','x');";
    for suffix in [
        "const {writeFileSync:require}=require('fs');",
        "const {readFileSync:module}=require('fs');",
        "const {readFileSync:exports}=require('fs');",
        "const {readFileSync:__dirname}=require('fs');",
        "const {readFileSync:__filename}=require('fs');",
        "const {readFileSync:eval}=require('fs');",
        "const {readFileSync:arguments}=require('fs');",
        "const {readFileSync:x,writeFileSync:x}=require('fs');",
        "const x='x'; const {writeFileSync:x}=require('fs');",
        "const {writeFileSync:x}=require('fs'); let x='x';",
        "const {writeFileSync:a,writeFileSync:b}=require('fs');",
        "let {writeFileSync:w}=require('fs');",
        "const {writeFileSync:w=unknown()}=require('fs');",
        "const {writeFileSync=unknown()}=require('fs');",
        "const {...fs}=require('fs');",
        "const {[key]:w}=require('fs');",
        "const {'writeFileSync':w}=require('fs');",
        "const {writeFileSync:{call:w}}=require('fs');",
        "const {writeFileSync:w}=fs;",
        "const {writeFileSync:w}=load('fs');",
        "const {unknown:w}=require('fs');",
        "const {}=require('fs');",
    ] {
        let report = forecast(&format!("{prefix}{suffix}"))?;
        assert!(
            report["edits"].as_array().expect("edits").is_empty(),
            "{suffix}"
        );
    }
    Ok(())
}

#[test]
fn destructured_functions_are_unavailable_before_initialization() -> TestResult {
    for source in [
        "w('out','x'); const {writeFileSync:w}=require('fs');",
        "const text=r('in','utf8'); const {readFileSync:r,writeFileSync:w}=require('fs'); w('out',text);",
    ] {
        let report = forecast(source)?;
        assert!(report["edits"].as_array().expect("edits").is_empty());
        assert_eq!(report["javascriptBarriers"][0]["reason"], "unknownName");
    }
    Ok(())
}

#[test]
fn function_assignment_and_dynamic_invocation_stop_successor_forecasts() -> TestResult {
    let prefix = "const {writeFileSync:w}=require('fs'); w('before','x');";
    for suffix in [
        "let alias='x'; alias=w; alias('after','y');",
        "w='x'; w('after','y');",
        "w.call('receiver','after','y');",
    ] {
        let report = forecast(&format!("{prefix}{suffix}"))?;
        assert_eq!(
            report["edits"].as_array().expect("edits").len(),
            1,
            "{suffix}"
        );
        assert!(
            report["edits"]
                .as_array()
                .expect("edits")
                .iter()
                .all(|edit| edit["details"]["prediction"]["target"]["path"] == "before")
        );
    }
    let report = forecast(&format!("{prefix} unknown(); w('after','y');"))?;
    assert_eq!(report["edits"].as_array().expect("edits").len(), 1);
    assert_eq!(report["edits"][0]["laterContext"], "opaqueSuffix");
    Ok(())
}

#[test]
fn empty_needle_substitutions_regex_and_nonliteral_arguments_remain_unsupported() -> TestResult {
    for expression in [
        "'aa'.replaceAll('','b')",
        "'aa'.replaceAll('a','$&')",
        "'aa'.replaceAll('a','$$')",
        "'aa'.replaceAll('a','$1')",
        "'aa'.replaceAll(/a/g,'b')",
        "'aa'.replaceAll('a',()=> 'b')",
        "'aa'.replaceAll('a')",
        "'aa'.replaceAll('a','b','extra')",
        "'aa'.replaceAll(require('fs').readFileSync('needle','utf8'),'b')",
    ] {
        let report = forecast(&format!(
            "const {{writeFileSync:w}}=require('fs'); w('out',{expression});"
        ))?;
        assert!(
            report["edits"].as_array().expect("edits").is_empty(),
            "{expression}"
        );
    }
    Ok(())
}
