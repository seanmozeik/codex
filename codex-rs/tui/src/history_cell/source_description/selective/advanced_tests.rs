//! Shared live/restored rendering tests use authored DTOs, never submitted code.

use super::super::HistoryCell;
use super::super::SourceDescriptionCell;
use crate::test_support::PathBufExt;
use crate::test_support::test_path_buf;
use codex_app_server_protocol::ThreadItem;
use codex_protocol::source_description::SourceDescription;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

fn raw(cell: &dyn HistoryCell) -> String {
    cell.raw_lines()
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

fn parity(value: Value, expected: &[&str]) {
    let description: SourceDescription = serde_json::from_value(value).expect("authored DTO");
    let live = SourceDescriptionCell::new("recipes".to_owned(), description.clone())
        .expect("presentable cell");
    let restored = crate::thread_transcript::thread_items_to_transcript_cells(
        None,
        &test_path_buf("/workspace").abs(),
        vec![ThreadItem::SourceDescription {
            id: "recipes".to_owned(),
            call_id: "call".to_owned(),
            description,
        }],
        crate::thread_transcript::RawReasoningVisibility::Hidden,
        None,
    );
    assert_eq!(restored.len(), 1);
    for width in [24, 80, 160] {
        assert_eq!(restored[0].display_lines(width), live.display_lines(width));
        assert_eq!(
            restored[0].transcript_lines(width),
            live.transcript_lines(width)
        );
    }
    for text in expected {
        assert!(raw(&live).contains(text), "missing {text}");
    }
    for text in ["Applied", "Observed diff", "Succeeded", "--- a/", "+++ b/"] {
        assert!(
            !raw(&live).contains(text),
            "unexpected outcome claim {text}"
        );
    }
}

#[test]
fn advanced_python_recipes_have_visible_conditions_and_no_executed_diff() {
    let fixture: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../protocol/tests/fixtures/source_description_selective.json"
    )))
    .expect("shared fixture");
    let old = &fixture["edits"][1]["details"]["prediction"]["operation"]["content"];
    let evidence = &old["evidence"];
    let read = &old["input"];
    let literal = json!({"type":"literal","value":"new","evidence":evidence});
    let index = json!({"type":"substringIndex","input":read,"needle":"x","start":{"type":"literal","value":-1,"evidence":evidence},"evidence":evidence});
    let recipes = [
        (
            json!({"type":"replaceText","input":read,"old":literal,"new":literal,"count":1,"evidence":evidence}),
            "Replace captured search text",
        ),
        (
            json!({"type":"slice","input":read,"start":index,"end":null,"evidence":evidence}),
            "Python index of \"x\", requiring a match",
        ),
        (
            json!({"type":"splitItem","input":read,"separator":"|","maxsplit":-1,"index":-1,"splitEvidence":evidence,"evidence":evidence}),
            "item -1, requiring a valid index",
        ),
        (
            json!({"type":"rstrip","input":read,"characters":"xy","evidence":evidence}),
            "Strip trailing characters in set \"xy\"",
        ),
        (
            json!({"type":"conditional","predicate":{"type":"endsWith","input":read,"suffix":"x","suffixEvidence":evidence,"evidence":evidence},"consequence":read,"alternative":literal,"evidence":evidence}),
            "Conditional text; evaluate only the selected branch",
        ),
    ];
    for (recipe, expected) in recipes {
        let mut value = fixture.clone();
        value["edits"][1]["details"]["prediction"]["operation"]["content"] = recipe;
        parity(
            value,
            &[
                expected,
                "execution and final file state are unobserved",
                "Read 2:0",
            ],
        );
    }
}

fn span(source: &str, needle: &str) -> Value {
    let start = source.find(needle).expect("authored span");
    let end = start + needle.len();
    json!({"sourceId":0,"startByte":start,"endByte":end,"startLine":source[..start].bytes().filter(|byte| *byte == b'\n').count()+1,"endLine":source[..end].bytes().filter(|byte| *byte == b'\n').count()+1,"startColumn":source[..start].rsplit('\n').next().expect("prefix").len()+1})
}

fn node_fixture(method: &str) -> Value {
    let source = format!(
        "const fs = require('node:fs');\nconst text = fs.readFileSync('note.txt', 'utf8');\nfs.writeFileSync('note.txt', text.{method}('old', 'new'), 'utf8');\n"
    );
    let read = span(&source, "fs.readFileSync('note.txt', 'utf8')");
    let operation = span(
        &source,
        &format!("fs.writeFileSync('note.txt', text.{method}('old', 'new'), 'utf8')"),
    );
    let recipe_type = match method {
        "replace" => "replaceFirst",
        "replaceAll" => "replaceAll",
        _ => panic!("authored replacement method"),
    };
    let assumptions = [
        "standardNodeCommonJsEvaluation",
        "canonicalNodeFs",
        "standardStringMethods",
        "noConcurrentMutation",
    ];
    let mut dependencies = assumptions
        .iter()
        .map(|assumption| json!({"type":"capability","assumption":assumption}))
        .collect::<Vec<_>>();
    dependencies.extend([
        json!({"type":"entryWorkingDirectory","cwd":"/workspace"}),
        json!({"type":"filesystemEpoch","epoch":0}),
        json!({"type":"readValue","readId":0,"evidence":read}),
    ]);
    let target = json!({"path":"note.txt","cwd":"/workspace"});
    json!({"disposition":"selectiveFindings","sources":[{"id":0,"language":"typeScript","parent":null,"text":source}],"edits":[{
        "ordinal":0,"basis":"standardApiForecast","evidence":operation,
        "details":{"kind":"javaScript","assumptions":assumptions,"prediction":{
            "ordinal":0,"evidence":operation,"target":target,
            "content":{"type":recipe_type,"input":{"type":"readUtf8","readId":0,"target":target,"evidence":read,"filesystemEpoch":0},"old":"old","new":"new","evidence":span(&source,&format!("text.{method}('old', 'new')"))},
            "dependencies":dependencies,"predecessorConditions":[read]
        }},"predecessorConditions":[],"carrierAssumptions":[],"laterContext":"unobserved"
    }],"barriers":[]})
}

#[test]
fn node_first_match_semantics_survive_live_and_restored_rendering() {
    parity(
        node_fixture("replace"),
        &[
            "Replace first \"old\" with literal \"new\"",
            "Node UTF-8 read 0:0",
            "Assume standard Node CommonJS evaluation",
            "Operation: synchronous Node text overwrite",
        ],
    );
}

#[test]
fn node_unmodeled_substitution_and_control_payloads_never_reach_the_terminal() {
    for replacement in [
        "$&",
        "\u{1b}[31m",
        "\0",
        "\u{202e}",
        "sk-abcdefghijklmnopqrstuvwx",
    ] {
        let mut value = node_fixture("replace");
        value["edits"][0]["details"]["prediction"]["content"]["new"] = json!(replacement);
        let description: SourceDescription = serde_json::from_value(value).expect("typed Node DTO");
        assert!(SourceDescriptionCell::new("node".to_owned(), description).is_none());
    }
}

#[test]
fn node_all_match_semantics_survive_live_and_restored_rendering() {
    parity(
        node_fixture("replaceAll"),
        &[
            "Replace all \"old\" with literal \"new\"",
            "Node UTF-8 read 0:0",
            "Requires earlier Node operation to complete normally",
            "execution and final file state are unobserved",
        ],
    );
    let description: SourceDescription =
        serde_json::from_value(node_fixture("replaceAll")).expect("authored all-match DTO");
    let live = SourceDescriptionCell::new("node-all".to_owned(), description)
        .expect("presentable all-match cell");
    let compact = live
        .display_lines(160)
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(compact.contains("replace all \"old\" with \"new\""));
}

#[test]
fn malformed_all_match_payload_is_rejected_before_live_or_restored_presentation() {
    for change in 0..4 {
        let mut value = node_fixture("replaceAll");
        let content = &mut value["edits"][0]["details"]["prediction"]["content"];
        match change {
            0 => content["old"] = json!(""),
            1 => content["new"] = json!("$&"),
            2 => content["new"] = json!("\r\n"),
            _ => content["evidence"]["startLine"] = json!(2),
        }
        let description: SourceDescription = serde_json::from_value(value).expect("typed DTO");
        assert!(SourceDescriptionCell::new("node-all".to_owned(), description.clone()).is_none());
        let restored = crate::thread_transcript::thread_items_to_transcript_cells(
            None,
            &test_path_buf("/workspace").abs(),
            vec![ThreadItem::SourceDescription {
                id: "node-all".to_owned(),
                call_id: "call".to_owned(),
                description,
            }],
            crate::thread_transcript::RawReasoningVisibility::Hidden,
            None,
        );
        assert!(restored.is_empty());
    }
}

#[test]
fn lazy_boolean_text_predicates_preserve_live_restored_parity_and_conditional_labels() {
    let mut value: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../protocol/tests/fixtures/source_description_selective.json"
    )))
    .expect("shared fixture");
    let old = &value["edits"][1]["details"]["prediction"]["operation"]["content"];
    let evidence = &old["evidence"];
    let read = &old["input"];
    let literal = json!({"type":"literal","value":"é","evidence":evidence});
    let content = json!({
        "type":"conditional",
        "predicate":{
            "type":"and",
            "left":{"type":"textContains","needle":literal,"haystack":read,"evidence":evidence},
            "right":{"type":"textEquals","left":read,"right":literal,"evidence":evidence},
            "evidence":evidence
        },
        "consequence":read,"alternative":literal,"evidence":evidence
    });
    value["edits"][1]["details"]["prediction"]["operation"]["content"] = content;
    parity(
        value,
        &[
            "Boolean and; evaluate right only if left is true",
            "Python text membership; needle then haystack",
            "Python text equality; left then right",
            "execution and final file state are unobserved",
        ],
    );
}

fn node_slice_fixture(end: Option<usize>) -> Value {
    let arguments = end.map_or_else(|| "0".to_owned(), |end| format!("0,{end}"));
    let expression = format!("text.slice({arguments})");
    let write = format!("fs.writeFileSync('note.txt',{expression})");
    let source =
        format!("const fs=require('fs'); const text=fs.readFileSync('note.txt','utf8'); {write};");
    let read = span(&source, "fs.readFileSync('note.txt','utf8')");
    let operation = span(&source, &write);
    let assumptions = [
        "standardNodeCommonJsEvaluation",
        "canonicalNodeFs",
        "standardStringMethods",
        "noConcurrentMutation",
    ];
    let mut dependencies = assumptions
        .iter()
        .map(|assumption| json!({"type":"capability","assumption":assumption}))
        .collect::<Vec<_>>();
    dependencies.extend([
        json!({"type":"entryWorkingDirectory","cwd":"/workspace"}),
        json!({"type":"filesystemEpoch","epoch":0}),
        json!({"type":"readValue","readId":0,"evidence":read}),
    ]);
    let target = json!({"path":"note.txt","cwd":"/workspace"});
    json!({"disposition":"selectiveFindings","sources":[{"id":0,"language":"typeScript","parent":null,"text":source}],"edits":[{
        "ordinal":0,"basis":"standardApiForecast","evidence":operation,
        "details":{"kind":"javaScript","assumptions":assumptions,"prediction":{
            "ordinal":0,"evidence":operation,"target":target,
            "content":{"type":"slice","input":{"type":"readUtf8","readId":0,"target":target,"evidence":read,"filesystemEpoch":0},"start":0,"end":end,"evidence":span(&source,&expression)},
            "dependencies":dependencies,"predecessorConditions":[read]
        }},"predecessorConditions":[],"carrierAssumptions":[],"laterContext":"unobserved"
    }],"barriers":[]})
}

#[test]
fn node_slice_discloses_utf16_units_in_live_and_restored_views() {
    for (end, label) in [
        (None, "Node UTF-16 slice 0..length"),
        (Some(2), "Node UTF-16 slice 0..2"),
    ] {
        let value = node_slice_fixture(end);
        parity(
            value.clone(),
            &[
                label,
                "clamp to length",
                "Node UTF-8 read 0:0",
                "Assume standard JavaScript string methods",
                "execution and final file state are unobserved",
            ],
        );
        let description = serde_json::from_value(value).expect("authored slice DTO");
        let live = SourceDescriptionCell::new("node-slice".to_owned(), description)
            .expect("presentable slice cell");
        let compact = live
            .display_lines(160)
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(compact.contains("Node write a UTF-16 slice"));
    }
}

#[test]
fn node_slice_missing_string_premise_is_rejected_before_presentation() {
    let mut value = node_slice_fixture(Some(2));
    value["edits"][0]["details"]["prediction"]["dependencies"]
        .as_array_mut()
        .expect("dependencies")
        .retain(|value| value["assumption"] != "standardStringMethods");
    let description: SourceDescription = serde_json::from_value(value).expect("typed slice DTO");
    assert!(SourceDescriptionCell::new("node-slice".to_owned(), description).is_none());
}

mod append;
