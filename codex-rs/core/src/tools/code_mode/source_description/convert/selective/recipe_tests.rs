//! Native parser-to-wire regressions; submitted Python and Node code is never run.

use super::activity;
use super::description;
use super::tests::analyze;
use super::wire;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::time::Duration;

#[test]
fn every_python_text_recipe_and_index_survives_native_conversion() {
    for (expression, expected) in [
        ("s[1:-1]", vec!["slice", "literal"]),
        (
            "s if 'é' in s else 'other'",
            vec!["conditional", "textContains"],
        ),
        (
            "s if s == 'é' else 'other'",
            vec!["conditional", "textEquals"],
        ),
        (
            "s if 'é' in s and s == 'é' else 'other'",
            vec!["conditional", "and", "textContains", "textEquals"],
        ),
        (
            "s if s.startswith('«') and s.endswith('»') else 'other'",
            vec!["conditional", "and", "startsWith", "endsWith"],
        ),
        (
            "s[s.index('BEGIN'):s.index('END', 1)]",
            vec!["slice", "substringIndex"],
        ),
        ("s.replace(s[1:3], 'new', 1)", vec!["replaceText", "slice"]),
        ("s.split('|', 2)[-1]", vec!["splitItem"]),
        ("s.rstrip('xy')", vec!["rstrip"]),
        ("s + '\\n'", vec!["concat"]),
        ("'|'.join([s, 'tail'])", vec!["concat", "read", "literal"]),
        (
            "s if s.endswith('\\n') else s + '\\n'",
            vec!["conditional", "endsWith"],
        ),
        (
            "s if s.startswith('BEGIN') else 'BEGIN' + s",
            vec!["conditional", "startsWith"],
        ),
    ] {
        let source = format!(
            "from pathlib import Path\np = Path('note.txt')\ns = p.read_text(encoding='utf-8')\np.write_text({expression}, encoding='utf-8')\n"
        );
        let converted = description(&analyze(&source));
        let encoded = serde_json::to_value(&converted).expect("wire serialization");
        assert_eq!(
            encoded["disposition"],
            json!("selectiveFindings"),
            "{expression}"
        );
        assert_eq!(
            encoded["edits"].as_array().expect("edits").len(),
            1,
            "{expression}"
        );
        for kind in expected {
            assert!(
                contains_type(
                    &encoded["edits"][0]["details"]["prediction"]["operation"]["content"],
                    kind
                ),
                "missing {kind}: {expression}"
            );
        }
        assert_eq!(
            serde_json::from_value::<wire::SourceDescription>(encoded).expect("round trip"),
            converted
        );
    }
}

fn contains_type(value: &Value, kind: &str) -> bool {
    match value {
        Value::Object(object) => {
            object.get("type").is_some_and(|value| value == kind)
                || object.values().any(|value| contains_type(value, kind))
        }
        Value::Array(values) => values.iter().any(|value| contains_type(value, kind)),
        _ => false,
    }
}

#[test]
fn captured_split_origin_can_precede_the_subscription() {
    let result = analyze(concat!(
        "from pathlib import Path\np = Path('note.txt')\n",
        "s = p.read_text(encoding='utf-8')\nparts = s.split('|')\n",
        "p.write_text(parts[0], encoding='utf-8')\n",
    ));
    let value = serde_json::to_value(description(&result)).expect("converted split");
    assert_eq!(value["disposition"], json!("selectiveFindings"));
    let content = &value["edits"][0]["details"]["prediction"]["operation"]["content"];
    assert_eq!(content["type"], json!("splitItem"));
    assert!(
        content["splitEvidence"]["endByte"].as_u64() < content["evidence"]["startByte"].as_u64()
    );
}

#[test]
fn loop_occurrences_preserve_repeated_spans_and_prior_iteration_conditions() {
    let result = analyze(concat!(
        "from pathlib import Path\n",
        "for name in ['first.txt', 'second.txt']:\n",
        "    p = Path(name)\n",
        "    s = p.read_text(encoding='utf-8')\n",
        "    p.write_text(s.rstrip('x'), encoding='utf-8')\n",
    ));
    let value = serde_json::to_value(description(&result)).expect("converted iterations");
    assert_eq!(value["disposition"], json!("selectiveFindings"));
    let edits = value["edits"].as_array().expect("edits");
    assert_eq!(edits.len(), 2);
    assert_eq!(edits[0]["evidence"], edits[1]["evidence"]);
    for (index, edit) in edits.iter().enumerate() {
        let prediction = &edit["details"]["prediction"];
        assert_eq!(prediction["iterationContext"][0]["index"], json!(index));
        assert_eq!(prediction["iterationContext"][0]["literalCount"], json!(2));
        assert_eq!(
            prediction["operation"]["content"]["input"]["readId"],
            json!(index)
        );
        assert_eq!(
            prediction["operation"]["content"]["input"]["filesystemEpoch"],
            json!(index)
        );
    }
}

#[test]
fn node_inline_reads_preserve_first_match_recipe_and_utf8_identity() {
    let source = "const fs = require('node:fs'); fs.writeFileSync('note.txt', fs.readFileSync('note.txt', 'utf8').replace('old', 'new') + '\\n', 'utf8');";
    let input = activity::SelectiveEditInput::from_node_commonjs(
        "node".into(),
        source,
        Some("/workspace"),
        activity::AnalysisLimits::new(32_768, Duration::from_secs(2)),
    )
    .expect("bounded Node source");
    let native = activity::Analyzer::new()
        .expect("native parsers")
        .forecast_edits(input);
    let converted = description(&native);
    let value = serde_json::to_value(&converted).expect("Node wire");
    assert_eq!(value["disposition"], json!("selectiveFindings"));
    assert_eq!(value["edits"].as_array().expect("edits").len(), 1);
    assert_eq!(value["edits"][0]["details"]["kind"], json!("javaScript"));
    let content = &value["edits"][0]["details"]["prediction"]["content"];
    for kind in ["concat", "replaceFirst", "readUtf8", "literal"] {
        assert!(contains_type(content, kind), "missing {kind}");
    }
    assert_eq!(
        serde_json::from_value::<wire::SourceDescription>(value).expect("Node round trip"),
        converted
    );
}

#[test]
fn node_destructured_functions_and_all_match_recipe_survive_conversion() {
    let source = "const {readFileSync: read, writeFileSync: write}=require('fs'); const text=read('in','utf8'); write('out',text.replaceAll('old','new'),'utf8');";
    let input = activity::SelectiveEditInput::from_node_commonjs(
        "node-all".into(),
        source,
        Some("/workspace"),
        activity::AnalysisLimits::new(32_768, Duration::from_secs(2)),
    )
    .expect("bounded Node source");
    let native = activity::Analyzer::new()
        .expect("native parsers")
        .forecast_edits(input);
    let converted = description(&native);
    assert_eq!(converted.clone().into_presentable(|_| true), converted);
    let value = serde_json::to_value(&converted).expect("Node wire");
    assert_eq!(value["disposition"], "selectiveFindings");
    assert_eq!(value["edits"].as_array().expect("edits").len(), 1);
    let prediction = &value["edits"][0]["details"]["prediction"];
    assert_eq!(prediction["content"]["type"], "replaceAll");
    assert_eq!(prediction["content"]["old"], "old");
    assert_eq!(prediction["content"]["new"], "new");
    assert_eq!(prediction["content"]["input"]["type"], "readUtf8");
    assert_eq!(prediction["content"]["input"]["readId"], 0);
    assert_eq!(prediction["content"]["input"]["filesystemEpoch"], 0);
    for name in ["read", "write"] {
        assert!(
            prediction["dependencies"]
                .as_array()
                .expect("dependencies")
                .iter()
                .any(|dependency| dependency["type"] == "binding" && dependency["name"] == name)
        );
    }
    assert_eq!(
        prediction["predecessorConditions"]
            .as_array()
            .expect("conditions")
            .len(),
        2
    );
    assert_eq!(
        serde_json::from_value::<wire::SourceDescription>(value).expect("round trip"),
        converted
    );
}

#[test]
fn captured_predicate_evidence_and_closed_rhs_survive_conversion_and_validation() {
    let source = concat!(
        "from pathlib import Path\ns=Path('note.txt').read_text(encoding='utf-8')\n",
        "start=s.startswith\ns='rebound'\np=start('é')\nsaved=p\np='rebound'\n",
        "Path('note.txt').write_text('yes' if saved and 'a' == 'a' else 'no',encoding='utf-8')",
    );
    let converted = description(&analyze(source));
    assert_eq!(converted.clone().into_presentable(|_| true), converted);
    let value = serde_json::to_value(&converted).expect("converted predicate");
    assert_eq!(value["disposition"], "selectiveFindings");
    let predicate =
        &value["edits"][0]["details"]["prediction"]["operation"]["content"]["predicate"];
    assert_eq!(predicate["type"], "and");
    assert_eq!(predicate["left"]["input"]["readId"], 0);
    assert_eq!(predicate["right"]["type"], "textEquals");
    assert_eq!(
        serde_json::from_value::<wire::SourceDescription>(value).expect("round trip"),
        converted
    );
}

#[test]
fn node_slice_preserves_utf16_bounds_read_identity_and_presentation_round_trip() {
    for (arguments, end) in [("1", None), ("0,2", Some(2))] {
        let source = format!(
            "const fs=require('fs'); const text=fs.readFileSync('in','utf8'); fs.writeFileSync('out',text.slice({arguments}));"
        );
        let input = activity::SelectiveEditInput::from_node_commonjs(
            "node-slice".into(),
            &source,
            Some("/workspace"),
            activity::AnalysisLimits::new(32_768, Duration::from_secs(2)),
        )
        .expect("bounded Node source");
        let native = activity::Analyzer::new()
            .expect("native parsers")
            .forecast_edits(input);
        let converted = description(&native);
        assert_eq!(converted.clone().into_presentable(|_| true), converted);
        let value = serde_json::to_value(&converted).expect("Node wire");
        assert_eq!(value["disposition"], "selectiveFindings");
        assert_eq!(value["edits"].as_array().expect("edits").len(), 1);
        let content = &value["edits"][0]["details"]["prediction"]["content"];
        assert_eq!(content["type"], "slice");
        assert_eq!(content["end"], json!(end));
        assert_eq!(content["input"]["readId"], 0);
        assert_eq!(content["input"]["filesystemEpoch"], 0);
        assert_eq!(
            serde_json::from_value::<wire::SourceDescription>(value).expect("round trip"),
            converted
        );
    }
}
