//! Authored DTO boundary tests; grammar/semantic proof belongs to the producer.

use crate::source_description::SourceDescription;
use crate::source_description::SourceDescriptionFallback;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

fn fixture() -> Value {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/source_description_selective.json"
    )))
    .expect("authored fixture")
}

fn accepted(value: Value) {
    let input: SourceDescription = serde_json::from_value(value).expect("typed recipe");
    assert_eq!(input.clone().into_presentable(|_| true), input);
}

fn rejected(value: Value) {
    let input: SourceDescription = serde_json::from_value(value).expect("typed recipe");
    assert_eq!(
        input.into_presentable(|_| true),
        SourceDescription::Fallback {
            reason: SourceDescriptionFallback::Presentation
        }
    );
}

fn content(value: &mut Value) -> &mut Value {
    &mut value["edits"][1]["details"]["prediction"]["operation"]["content"]
}

fn recipes() -> Vec<Value> {
    let value = fixture();
    let old = &value["edits"][1]["details"]["prediction"]["operation"]["content"];
    let evidence = &old["evidence"];
    let read = &old["input"];
    let literal = json!({"type":"literal","value":"new","evidence":evidence});
    let index = json!({"type":"substringIndex","input":read,"needle":"x","start":{"type":"literal","value":-1,"evidence":evidence},"evidence":evidence});
    vec![
        json!({"type":"replaceText","input":read,"old":literal,"new":literal,"count":2,"evidence":evidence}),
        json!({"type":"slice","input":read,"start":index,"end":null,"evidence":evidence}),
        json!({"type":"splitItem","input":read,"separator":"|","maxsplit":-1,"index":-1,"splitEvidence":evidence,"evidence":evidence}),
        json!({"type":"rstrip","input":read,"characters":"xy","evidence":evidence}),
        json!({"type":"conditional","predicate":{"type":"startsWith","input":read,"prefix":"x","prefixEvidence":evidence,"evidence":evidence},"consequence":read,"alternative":literal,"evidence":evidence}),
        json!({"type":"conditional","predicate":{"type":"endsWith","input":read,"suffix":"x","suffixEvidence":evidence,"evidence":evidence},"consequence":literal,"alternative":read,"evidence":evidence}),
    ]
}

#[test]
fn new_text_integer_and_predicate_shapes_preserve_read_provenance() {
    for recipe in recipes() {
        let mut value = fixture();
        *content(&mut value) = recipe;
        accepted(value);
    }
}

#[test]
fn every_branch_and_argument_is_subject_to_disclosure_and_evidence_checks() {
    for change in 0..7 {
        let mut value = fixture();
        *content(&mut value) = recipes().pop().expect("conditional");
        match change {
            0 => content(&mut value)["alternative"]["evidence"]["sourceId"] = json!(1),
            1 => content(&mut value)["predicate"]["suffixEvidence"]["sourceId"] = json!(1),
            2 => content(&mut value)["consequence"]["value"] = json!("\u{1b}[31m"),
            3 => content(&mut value)["predicate"]["suffix"] = json!("\u{202e}"),
            4 => content(&mut value)["alternative"]["filesystemEpoch"] = json!(4),
            5 => content(&mut value)["predicate"]["input"]["readId"] = json!(41),
            _ => {
                value["edits"][1]["details"]["prediction"]["dependencies"]
                    .as_array_mut()
                    .expect("dependencies")
                    .retain(|dependency| dependency["type"] != "readValue");
            }
        }
        rejected(value);
    }
}

#[test]
fn combined_text_index_predicate_depth_and_exact_wire_integers_are_bounded() {
    let mut value = fixture();
    let evidence = content(&mut value)["evidence"].clone();
    let mut nested = recipes()[1].clone();
    for _ in 0..34 {
        nested = json!({"type":"slice","input":nested,"start":{"type":"literal","value":0,"evidence":evidence},"end":null,"evidence":evidence});
    }
    *content(&mut value) = nested;
    rejected(value);
    for change in 0..3 {
        let mut value = fixture();
        *content(&mut value) = recipes()[2].clone();
        match change {
            0 => content(&mut value)["separator"] = json!(""),
            1 => content(&mut value)["index"] = json!(9_007_199_254_740_992i64),
            _ => content(&mut value)["splitEvidence"]["sourceId"] = json!(1),
        }
        rejected(value);
    }
}

fn span(source: &str, needle: &str) -> Value {
    let start = source.find(needle).expect("authored span");
    let end = start + needle.len();
    json!({"sourceId":0,"startByte":start,"endByte":end,"startLine":source[..start].bytes().filter(|byte| *byte == b'\n').count()+1,"endLine":source[..end].bytes().filter(|byte| *byte == b'\n').count()+1,"startColumn":source[..start].rsplit('\n').next().expect("prefix").len()+1})
}

fn loop_fixture() -> Value {
    let source = "from pathlib import Path\nfor name in ['first.txt', 'second.txt']:\n    Path(name).write_text('new', encoding='utf-8')\n";
    let operation = span(source, "Path(name).write_text('new', encoding='utf-8')");
    let iteration = span(
        source,
        "for name in ['first.txt', 'second.txt']:\n    Path(name).write_text('new', encoding='utf-8')",
    );
    let assumptions = json!([
        "standardPythonEvaluation",
        "canonicalPathlib",
        "noConcurrentMutation"
    ]);
    let edits = ["first.txt", "second.txt"].into_iter().enumerate().map(|(ordinal, path)| {
        json!({"ordinal":ordinal,"basis":"standardApiForecast","evidence":operation,"details":{"kind":"python","assumptions":assumptions,"prediction":{
            "ordinal":ordinal,"iterationContext":[{"evidence":iteration,"index":ordinal,"literalCount":2}],
            "evidence":operation,"target":{"path":path,"cwd":null},
            "operation":{"type":"writeText","content":{"type":"literal","value":"new","evidence":span(source,"'new'")},"encoding":"utf8"},
            "dependencies":[{"type":"capability","assumption":"standardPythonEvaluation"},{"type":"capability","assumption":"canonicalPathlib"},{"type":"capability","assumption":"noConcurrentMutation"},{"type":"filesystemEpoch","epoch":ordinal},{"type":"entryWorkingDirectory","cwd":null}],
            "predecessorConditions":if ordinal == 0 {vec![]} else {vec![operation.clone()]},"finalState":"notEstablished"
        }},"predecessorConditions":[],"carrierAssumptions":[],"laterContext":"unobserved"})
    }).collect::<Vec<_>>();
    json!({"disposition":"selectiveFindings","sources":[{"id":0,"language":"python","parent":null,"text":source}],"edits":edits,"barriers":[]})
}

#[test]
fn repeated_sink_predecessor_requires_bounded_enclosing_iteration_context() {
    accepted(loop_fixture());
    for change in 0..5 {
        let mut value = loop_fixture();
        let prediction = &mut value["edits"][1]["details"]["prediction"];
        match change {
            0 => prediction["iterationContext"] = json!([]),
            1 => prediction["iterationContext"][0]["index"] = json!(0),
            2 => prediction["iterationContext"][0]["index"] = json!(2),
            3 => prediction["iterationContext"][0]["literalCount"] = json!(65),
            _ => prediction["iterationContext"][0]["evidence"] = prediction["evidence"].clone(),
        }
        rejected(value);
    }
}

fn conjunction_recipe() -> Value {
    let value = fixture();
    let old = &value["edits"][1]["details"]["prediction"]["operation"]["content"];
    let evidence = &old["evidence"];
    let read = &old["input"];
    let literal = json!({"type":"literal","value":"é","evidence":evidence});
    json!({
        "type":"conditional",
        "predicate":{
            "type":"and",
            "left":{"type":"textEquals","left":literal,"right":literal,"evidence":evidence},
            "right":{"type":"textContains","needle":literal,"haystack":read,"evidence":evidence},
            "evidence":evidence
        },
        "consequence":read,"alternative":literal,"evidence":evidence
    })
}

#[test]
fn closed_text_comparisons_and_boolean_conjunction_are_presentable() {
    let mut value = fixture();
    *content(&mut value) = conjunction_recipe();
    accepted(value);
}

#[test]
fn conjunction_rhs_disclosure_evidence_reads_and_capabilities_are_never_skipped() {
    for change in 0..7 {
        let mut value = fixture();
        *content(&mut value) = conjunction_recipe();
        // The false left side does not waive validation of any RHS field.
        content(&mut value)["predicate"]["left"]["right"]["value"] = json!("different");
        let rhs = &mut content(&mut value)["predicate"]["right"];
        match change {
            0 => rhs["needle"]["value"] = json!("\u{1b}[31m"),
            1 => rhs["evidence"]["sourceId"] = json!(1),
            2 => rhs["haystack"]["readId"] = json!(999),
            3 => rhs["needle"]["evidence"]["endByte"] = json!(999_999),
            4 => rhs["haystack"]["filesystemEpoch"] = json!(8),
            5 => {
                value["edits"][1]["details"]["prediction"]["dependencies"]
                    .as_array_mut()
                    .expect("dependencies")
                    .retain(|dependency| dependency["assumption"] != "standardTextMethods");
            }
            _ => {
                value["edits"][1]["details"]["prediction"]["dependencies"]
                    .as_array_mut()
                    .expect("dependencies")
                    .retain(|dependency| dependency["type"] != "readValue");
            }
        }
        rejected(value);
    }
}

#[test]
fn conjunction_recursion_and_payloads_obey_the_shared_wire_budget() {
    let mut value = fixture();
    *content(&mut value) = conjunction_recipe();
    let evidence = content(&mut value)["evidence"].clone();
    let leaf = content(&mut value)["predicate"]["left"].clone();
    let mut nested = leaf.clone();
    for _ in 0..34 {
        nested = json!({"type":"and","left":leaf,"right":nested,"evidence":evidence});
    }
    content(&mut value)["predicate"] = nested;
    rejected(value);

    let mut value = fixture();
    *content(&mut value) = conjunction_recipe();
    content(&mut value)["predicate"]["right"]["needle"]["value"] = json!("x".repeat(1_048_577));
    rejected(value);
}

#[test]
fn unknown_predicate_shapes_and_extra_fields_cannot_deserialize() {
    for change in 0..2 {
        let mut value = fixture();
        *content(&mut value) = conjunction_recipe();
        if change == 0 {
            content(&mut value)["predicate"]["type"] = json!("or");
        } else {
            content(&mut value)["predicate"]["coerce"] = json!(true);
        }
        assert!(serde_json::from_value::<SourceDescription>(value).is_err());
    }
}
