//! Authored source-only Node fixtures. Captured JavaScript is never executed.

use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::Language;
use codex_code_activity::SelectiveEditInput;
use pretty_assertions::assert_eq;
use serde_json::Value;
use std::time::Duration;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn forecast(source: &str) -> TestResult<Value> {
    let shell = format!("node <<'NODE_SOURCE'\n{source}\nNODE_SOURCE\n");
    let input = SelectiveEditInput::from_source(
        "authored-node".into(),
        Language::Shell,
        &shell,
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
fn literal_write_keeps_node_capabilities_and_exact_invocation() -> TestResult {
    let source = "const fs = require('node:fs'); fs.writeFileSync('out', 'ink\\n😀');";
    let report = forecast(source)?;
    assert_eq!(report["disposition"], "analyzed");
    assert_eq!(report["edits"].as_array().expect("edits").len(), 1);
    let edit = prediction(&report, 0);
    assert_eq!(report["edits"][0]["details"]["kind"], "javaScript");
    assert_eq!(edit["target"]["path"], "out");
    assert_eq!(edit["target"]["cwd"], "/workspace");
    assert_eq!(edit["content"]["value"], "ink\n😀");
    assert_eq!(
        span_text(source, &edit["evidence"])?,
        "fs.writeFileSync('out', 'ink\\n😀')"
    );
    let dependencies = edit["dependencies"].as_array().expect("dependencies");
    for assumption in [
        "standardNodeCommonJsEvaluation",
        "canonicalNodeFs",
        "standardStringMethods",
        "noConcurrentMutation",
    ] {
        assert!(
            dependencies
                .iter()
                .any(|dependency| dependency["type"] == "capability"
                    && dependency["assumption"] == assumption)
        );
    }
    assert!(
        report["edits"][0]["carrierAssumptions"]
            .as_array()
            .expect("carriers")
            .iter()
            .any(|carrier| carrier["basis"] == "standardNodeCommonJsCli")
    );
    Ok(())
}

#[test]
fn text_alias_preserves_read_and_binding_versions_after_reassignment() -> TestResult {
    let source = "const fs=require('fs'); let data=fs.readFileSync('input','utf8'); const saved=data; data='other'; fs.writeFileSync('out',saved.replace('old','new'),'utf8');";
    let report = forecast(source)?;
    let edit = prediction(&report, 0);
    assert_eq!(edit["content"]["type"], "replaceFirst");
    assert_eq!(edit["content"]["input"]["type"], "readUtf8");
    assert_eq!(edit["content"]["input"]["readId"], 0);
    assert_eq!(edit["content"]["input"]["target"]["path"], "input");
    assert_eq!(edit["content"]["input"]["filesystemEpoch"], 0);
    assert_eq!(edit["content"]["old"], "old");
    assert_eq!(edit["content"]["new"], "new");
    let dependencies = edit["dependencies"].as_array().expect("dependencies");
    assert!(
        dependencies
            .iter()
            .any(|d| d["type"] == "binding" && d["name"] == "data" && d["version"] == 2)
    );
    assert!(
        dependencies
            .iter()
            .all(|d| d["type"] != "binding" || d["name"] != "data" || d["version"] != 4)
    );
    assert!(
        edit["predecessorConditions"]
            .as_array()
            .expect("predecessors")
            .iter()
            .map(|span| span_text(source, span))
            .collect::<TestResult<Vec<_>>>()?
            .contains(&"fs.readFileSync('input','utf8')")
    );
    Ok(())
}

#[test]
fn module_alias_and_string_only_concatenation_keep_literal_target() -> TestResult {
    let report = forecast(
        "const fs=require('node:fs'); const alias=fs; const folder='dir/'; const text='left'+'right'; alias.writeFileSync(folder+'out', text, 'utf8');",
    )?;
    let edit = prediction(&report, 0);
    assert_eq!(edit["target"]["path"], "dir/out");
    assert_eq!(edit["content"]["type"], "concat");
    assert_eq!(edit["content"]["left"]["value"], "left");
    assert_eq!(edit["content"]["right"]["value"], "right");
    Ok(())
}

#[test]
fn captured_read_epoch_does_not_advance_with_later_writes() -> TestResult {
    let report = forecast(
        "const fs=require('fs'); const saved=fs.readFileSync('a','utf8'); fs.writeFileSync('a','changed'); fs.writeFileSync('b',saved);",
    )?;
    assert_eq!(report["edits"].as_array().expect("edits").len(), 2);
    let edit = prediction(&report, 1);
    assert_eq!(edit["ordinal"], 1);
    assert_eq!(edit["content"]["filesystemEpoch"], 0);
    assert!(
        edit["dependencies"]
            .as_array()
            .expect("dependencies")
            .iter()
            .any(|d| d["type"] == "filesystemEpoch" && d["epoch"] == 1)
    );
    assert!(
        edit["dependencies"]
            .as_array()
            .expect("dependencies")
            .iter()
            .any(|d| d["type"] == "filesystemEpoch" && d["epoch"] == 0)
    );
    Ok(())
}

#[test]
fn unknown_execution_keeps_prefix_but_never_rescues_successors() -> TestResult {
    let report = forecast(
        "require('fs').writeFileSync('before','x'); unknown(); require('fs').writeFileSync('after','y');",
    )?;
    assert_eq!(report["edits"].as_array().expect("edits").len(), 1);
    assert_eq!(prediction(&report, 0)["target"]["path"], "before");
    assert_eq!(report["edits"][0]["laterContext"], "opaqueSuffix");
    assert_eq!(report["javascriptBarriers"][0]["reason"], "unmodeledCall");
    let before = forecast("unknown(); require('fs').writeFileSync('after','y');")?;
    assert!(before["edits"].as_array().expect("edits").is_empty());
    Ok(())
}

#[test]
fn duplicate_shadowing_and_unsupported_scope_refuse_the_whole_source() -> TestResult {
    let prefix = "const fs=require('fs'); fs.writeFileSync('out','x');";
    for suffix in [
        "const fs='again';",
        "const require='shadow';",
        "var require;",
        "function require() {}",
        "{ const local='x'; }",
        "import other from 'fs';",
        "let typed: string='x';",
        "await unknown();",
        "const bad='\\uD800';",
        "fs.writeFileSync=other;",
        "const bad='x';\u{0085}const later='y';",
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
fn lexical_initialization_order_prevents_tdz_writes() -> TestResult {
    for source in [
        "fs.writeFileSync('out','x'); const fs=require('fs');",
        "const fs=require('fs'); fs.writeFileSync('out',data); const data='x';",
        "const fs=require('fs'); const saved=data; const data='x'; fs.writeFileSync('out',saved);",
    ] {
        let report = forecast(source)?;
        assert!(
            report["edits"].as_array().expect("edits").is_empty(),
            "{source}"
        );
        assert_eq!(report["javascriptBarriers"][0]["reason"], "unknownName");
    }
    Ok(())
}

#[test]
fn replacement_tokens_regex_and_callbacks_are_never_python_replace() -> TestResult {
    for replacement in ["'$$'", "'$&'", "'$`'", "\"$'\"", "'$1'", "'$<name>'"] {
        let report = forecast(&format!(
            "require('fs').writeFileSync('out','aa'.replace('a',{replacement}));"
        ))?;
        assert!(
            report["edits"].as_array().expect("edits").is_empty(),
            "{replacement}"
        );
    }
    for expression in [
        "'aa'.replace(/a/g,'b')",
        "'aa'.replace('a',()=> 'b')",
        "'aa'.replaceAll('','b')",
    ] {
        let report = forecast(&format!("require('fs').writeFileSync('out',{expression});"))?;
        assert!(
            report["edits"].as_array().expect("edits").is_empty(),
            "{expression}"
        );
    }
    let empty = forecast("require('fs').writeFileSync('out','aa'.replace('','b'));")?;
    assert_eq!(prediction(&empty, 0)["content"]["type"], "replaceFirst");
    assert_eq!(prediction(&empty, 0)["content"]["old"], "");
    Ok(())
}

#[test]
fn empty_path_is_a_local_barrier_that_preserves_a_valid_prefix() -> TestResult {
    let report = forecast(
        "require('fs').writeFileSync('before','x'); require('fs').writeFileSync('','y');",
    )?;
    assert_eq!(report["edits"].as_array().expect("edits").len(), 1);
    assert_eq!(
        report["javascriptBarriers"][0]["reason"],
        "unsupportedOperation"
    );
    assert_eq!(report["edits"][0]["laterContext"], "opaqueSuffix");
    Ok(())
}

#[test]
fn encoding_getter_and_dynamic_dispatch_forms_do_not_publish_writes() -> TestResult {
    for source in [
        "require('fs').writeFileSync('out','x','base64');",
        "require('fs').writeFileSync('out','x',{encoding:'utf8'});",
        "require('fs')['writeFileSync']('out','x');",
        "require('fs')?.writeFileSync('out','x');",
        "const fs=require('fs'); fs.writeFileSync('out',fs.readFileSync('a'));",
        "require('fs').writeFileSync('out',{toString(){return 'x'}});",
    ] {
        let report = forecast(source)?;
        assert!(
            report["edits"].as_array().expect("edits").is_empty(),
            "{source}"
        );
    }
    Ok(())
}

#[test]
fn expression_growth_discards_partial_output_at_the_resource_boundary() -> TestResult {
    let mut source = "require('fs').writeFileSync('before','x'); const text0='x';".to_owned();
    for index in 1..20 {
        source.push_str(&format!(
            "const text{index}=text{}+text{};",
            index - 1,
            index - 1
        ));
    }
    source.push_str("require('fs').writeFileSync('after',text19);");
    let report = forecast(&source)?;
    assert!(report["edits"].as_array().expect("edits").is_empty());
    assert!(report["disposition"].is_object());
    Ok(())
}

#[test]
fn typescript_grammar_does_not_select_node_execution() {
    let input = SelectiveEditInput::from_source(
        "code-mode-is-distinct".into(),
        Language::TypeScript,
        "require('fs').writeFileSync('out','x');",
        None,
        AnalysisLimits::new(32_768, Duration::from_secs(2)),
    )
    .expect("bounded input");
    let mut analyzer = Analyzer::new().expect("cached parsers");
    assert!(analyzer.forecast_edits(input).edits().is_empty());
}
