use crate::source_description::SourceDescription;
use crate::source_description::SourceDescriptionFallback;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

fn fixture() -> Value {
    let mut value: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/source_description_selective.json"
    )))
    .expect("authored selective fixture");
    let mut edit = value["edits"][1].clone();
    edit["ordinal"] = json!(0);
    edit["predecessorConditions"] = json!([]);
    edit["snapshot"] = json!({
        "basis":"preExecutionSnapshotConditional", "scope":"operationPoint",
        "sourceId":2, "filesystemEpoch":0,
        "before":"old\n", "after":"new\n",
        "unifiedDiff":"--- supplied-before\n+++ conditional-after\n@@ -1,1 +1,1 @@\n-old\n+new\n",
        "readBindings":[{
            "sourceId":2, "readId":0, "filesystemEpoch":0,
            "target":{"path":"note.txt","cwd":"/workspace"},
            "encoding":"utf8", "newline":"pythonUniversal"
        }],
        "textProfile":{"readNewline":"pythonUniversal","writeNewline":"lf","runtimeDefaultEncoding":null},
        "environmentId":"local", "resolvedCwd":"/workspace",
        "resolvedTarget":"/workspace/note.txt", "replayOrdinals":[]
    });
    value["edits"] = json!([edit]);
    value
}

fn typed(value: Value) -> SourceDescription {
    serde_json::from_value(value).expect("typed snapshot fixture")
}

fn rejected(value: Value) {
    assert_eq!(
        typed(value).into_presentable(|_| true),
        SourceDescription::Fallback {
            reason: SourceDescriptionFallback::Presentation,
        }
    );
}

#[test]
fn snapshot_round_trip_preserves_conditional_basis_and_read_identity() {
    let value = fixture();
    let description = typed(value.clone());
    assert_eq!(description.clone().into_presentable(|_| true), description);
    assert_eq!(serde_json::to_value(description).expect("serialize"), value);
}

#[test]
fn mismatched_source_epoch_read_and_profile_are_rejected() {
    for change in 0..8 {
        let mut value = fixture();
        let snapshot = &mut value["edits"][0]["snapshot"];
        match change {
            0 => snapshot["sourceId"] = json!(0),
            1 => snapshot["filesystemEpoch"] = json!(1),
            2 => snapshot["readBindings"][0]["readId"] = json!(9),
            3 => snapshot["readBindings"][0]["target"]["path"] = json!("alias.txt"),
            4 => snapshot["readBindings"][0]["filesystemEpoch"] = json!(1),
            5 => snapshot["textProfile"]["writeNewline"] = json!("unspecified"),
            6 => snapshot["textProfile"]["runtimeDefaultEncoding"] = json!("utf8"),
            _ => {
                snapshot["readBindings"] = json!([
                    snapshot["readBindings"][0].clone(),
                    snapshot["readBindings"][0].clone()
                ])
            }
        }
        rejected(value);
    }
}

#[test]
fn snapshot_diff_must_reconstruct_the_serialized_endpoints() {
    for diff in [
        "--- supplied-before\n+++ conditional-after\n@@ -1,1 +1,1 @@\n-old\n+wrong\n",
        "--- supplied-before\n+++ conditional-after\n@@ -1,2 +1,1 @@\n-old\n+new\n",
        "--- supplied-before\n+++ conditional-after\n@@ -1,1 +1,1 @@\néold\n+new\n",
        "--- /dev/null\n+++ conditional-after\n@@ -1,1 +1,1 @@\n-old\n+new\n",
        "--- supplied-before\n+++ conditional-after\n@@ -1,1 +1,1 @@\n-old\n+new\nextra\n",
    ] {
        let mut value = fixture();
        value["edits"][0]["snapshot"]["unifiedDiff"] = json!(diff);
        rejected(value);
    }
}

#[test]
fn unresolved_python_default_codec_cannot_back_an_exact_snapshot() {
    let mut value = fixture();
    value["edits"][0]["details"]["prediction"]["operation"]["encoding"] = json!("runtimeDefault");
    rejected(value);
    let mut value = fixture();
    value["edits"][0]["details"]["prediction"]["operation"]["content"]["input"]["encoding"] =
        json!("runtimeDefault");
    value["edits"][0]["snapshot"]["readBindings"][0]["encoding"] = json!("runtimeDefault");
    rejected(value);
}

#[test]
fn missing_final_newlines_are_preserved_by_diff_validation() {
    let mut value = fixture();
    value["edits"][0]["snapshot"]["before"] = json!("old");
    value["edits"][0]["snapshot"]["after"] = json!("new");
    value["edits"][0]["snapshot"]["unifiedDiff"] = json!(
        "--- supplied-before\n+++ conditional-after\n@@ -1,1 +1,1 @@\n-old\n\\ No newline at end of file\n+new\n\\ No newline at end of file\n"
    );
    let input = typed(value);
    assert_eq!(input.clone().into_presentable(|_| true), input);
}

#[test]
fn snapshot_disclosure_covers_all_new_strings_without_redaction() {
    for blocked in ["old\n", "new\n", "local", "/workspace/note.txt"] {
        assert_eq!(
            typed(fixture()).into_presentable(|text| text != blocked),
            SourceDescription::Fallback {
                reason: SourceDescriptionFallback::Presentation
            }
        );
    }
    for invalid in ["\r\n", "\0", "\u{1b}[31m", "\u{202e}text"] {
        let mut value = fixture();
        value["edits"][0]["snapshot"]["after"] = json!(invalid);
        rejected(value);
    }
}

#[test]
fn oversized_endpoints_and_unbacked_replay_are_rejected() {
    let mut value = fixture();
    value["edits"][0]["snapshot"]["after"] = json!("x".repeat(65_537));
    rejected(value);
    let mut value = fixture();
    value["edits"][0]["snapshot"]["replayOrdinals"] = json!([0]);
    rejected(value);
}

#[test]
fn snapshot_cannot_claim_an_observed_basis_or_applied_status() {
    for field in ["basis", "applied"] {
        let mut value = fixture();
        value["edits"][0]["snapshot"][field] = json!("observed");
        assert!(serde_json::from_value::<SourceDescription>(value).is_err());
    }
}

fn span(source: &str, needle: &str) -> Value {
    let start = source.find(needle).expect("authored source span");
    let end = start + needle.len();
    json!({
        "sourceId":0, "startByte":start, "endByte":end,
        "startLine":source[..start].bytes().filter(|byte| *byte == b'\n').count()+1,
        "endLine":source[..end].bytes().filter(|byte| *byte == b'\n').count()+1,
        "startColumn":source[..start].rsplit('\n').next().expect("prefix").len()+1
    })
}

fn replay_fixture() -> Value {
    let source = concat!(
        "from pathlib import Path\np = Path('note.txt')\n",
        "p.write_text('a\\n', encoding='utf-8')\n",
        "p.write_text('b\\n', encoding='utf-8')\n",
        "p.write_text('c\\n', encoding='utf-8')\n"
    );
    let mut edits = Vec::new();
    let mut before = "old\n".to_owned();
    let mut predecessors = Vec::new();
    for (ordinal, letter) in ['a', 'b', 'c'].into_iter().enumerate() {
        let call = format!("p.write_text('{letter}\\n', encoding='utf-8')");
        let evidence = span(source, &call);
        let after = format!("{letter}\n");
        let mut snapshot = fixture()["edits"][0]["snapshot"].clone();
        snapshot["sourceId"] = json!(0);
        snapshot["filesystemEpoch"] = json!(ordinal);
        snapshot["before"] = json!(before);
        snapshot["after"] = json!(after);
        snapshot["readBindings"] = json!([]);
        snapshot["replayOrdinals"] = json!((0..ordinal).collect::<Vec<_>>());
        snapshot["unifiedDiff"] = json!(format!(
            "--- supplied-before\n+++ conditional-after\n@@ -1,1 +1,1 @@\n-{before}+{after}"
        ));
        edits.push(json!({
            "ordinal":ordinal, "basis":"standardApiForecast", "evidence":evidence,
            "details":{"kind":"python", "assumptions":[
                "standardPythonEvaluation", "canonicalPathlib", "noConcurrentMutation"
            ], "prediction":{
                "ordinal":ordinal, "evidence":evidence,
                "target":{"path":"note.txt","cwd":"/workspace"},
                "operation":{"type":"writeText","encoding":"utf8","content":{
                    "type":"literal", "value":after,
                    "evidence":span(source, &format!("'{letter}\\n'"))
                }},
                "dependencies":[
                    {"type":"capability","assumption":"standardPythonEvaluation"},
                    {"type":"capability","assumption":"canonicalPathlib"},
                    {"type":"capability","assumption":"noConcurrentMutation"},
                    {"type":"filesystemEpoch","epoch":ordinal},
                    {"type":"entryWorkingDirectory","cwd":"/workspace"},
                    {"type":"binding","name":"p","version":1,"evidence":span(source,"p = Path('note.txt')")}
                ],
                "predecessorConditions":predecessors, "finalState":"notEstablished"
            }},
            "predecessorConditions":[], "carrierAssumptions":[],
            "laterContext":"unobserved", "snapshot":snapshot
        }));
        predecessors.push(evidence);
        before = after;
    }
    json!({"disposition":"selectiveFindings","sources":[{
        "id":0,"language":"python","parent":null,"text":source
    }],"edits":edits,"barriers":[]})
}

#[test]
fn exact_three_operation_snapshot_chain_is_presentable() {
    let input = typed(replay_fixture());
    assert_eq!(input.clone().into_presentable(|_| true), input);
}

#[test]
fn replay_requires_each_predecessor_and_consistent_capture_context() {
    for change in 0..5 {
        let mut value = replay_fixture();
        match change {
            0 => value["edits"][0]
                .as_object_mut()
                .expect("edit")
                .remove("snapshot"),
            1 => {
                value["edits"][1]["snapshot"]["before"] = json!("wrong\n");
                value["edits"][1]["snapshot"]["unifiedDiff"] = json!(
                    "--- supplied-before\n+++ conditional-after\n@@ -1,1 +1,1 @@\n-wrong\n+b\n"
                );
                None
            }
            2 => {
                value["edits"][1]["snapshot"]["resolvedTarget"] = json!("/workspace/other.txt");
                None
            }
            3 => {
                value["edits"][1]["snapshot"]["environmentId"] = json!("remote");
                None
            }
            _ => {
                value["edits"][2]["snapshot"]["replayOrdinals"] = json!([1, 0]);
                None
            }
        };
        rejected(value);
    }
}

#[test]
fn source_declared_profile_preserves_concrete_read_policy() {
    for policy in ["universalTranslate", "preserve"] {
        let mut value = fixture();
        value["edits"][0]["snapshot"]["textProfile"]["readNewline"] = json!("sourceDeclared");
        value["edits"][0]["details"]["prediction"]["operation"]["content"]["input"]["newline"] =
            json!(policy);
        let concrete = if policy == "preserve" {
            "preserve"
        } else {
            "pythonUniversal"
        };
        value["edits"][0]["snapshot"]["readBindings"][0]["newline"] = json!(concrete);
        let description = typed(value.clone());
        assert_eq!(description.clone().into_presentable(|_| true), description);
        value["edits"][0]["snapshot"]["readBindings"][0]["newline"] = json!("sourceDeclared");
        rejected(value);
    }
}

#[test]
fn identical_read_id_cannot_change_newline_policy_in_one_recipe() {
    let mut value = fixture();
    let mut read =
        value["edits"][0]["details"]["prediction"]["operation"]["content"]["input"].clone();
    let original = read.clone();
    read["newline"] = json!("preserve");
    let evidence =
        value["edits"][0]["details"]["prediction"]["operation"]["content"]["evidence"].clone();
    value["edits"][0]["details"]["prediction"]["operation"]["content"] =
        json!({"type":"concat","left":original,"right":read,"evidence":evidence});
    rejected(value);
}
