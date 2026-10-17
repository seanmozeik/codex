//! Authored wire fixtures; no represented Python is executed.
use crate::source_description::SourceDescription;
use crate::source_description::SourceDescriptionFallback;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

fn source_span(source: &str, needle: &str) -> Value {
    let start = source.find(needle).expect("authored span");
    let prefix = &source[..start];
    let line = prefix.bytes().filter(|b| *b == b'\n').count() + 1;
    let column = prefix.rsplit('\n').next().expect("prefix").len() + 1;
    json!({"sourceId":0,"startByte":start,"endByte":start+needle.len(),"startLine":line,"endLine":line,"startColumn":column})
}

fn fixture() -> Value {
    let source = "f = open('note.txt', 'r+', encoding='utf-8', newline='')\nf.write('é\\n')\n";
    let span = |needle: &str| source_span(source, needle);
    let open = span("open('note.txt', 'r+', encoding='utf-8', newline='')");
    let write = span("f.write('é\\n')");
    let literal = span("'é\\n'");
    let assumptions = json!([
        "standardPythonEvaluation",
        "standardBuiltinOpen",
        "noConcurrentMutation"
    ]);
    json!({
        "disposition":"selectiveFindings", "sources":[{"id":0,"language":"python","parent":null,"text":source}],
        "edits":[{
            "ordinal":0,"basis":"standardApiForecast","evidence":write,
            "details":{"kind":"python","assumptions":assumptions,"prediction":{
                "ordinal":0,"evidence":write,"target":{"path":"note.txt","cwd":"/workspace"},
                "operation":{"type":"logicalWrite","streamId":0,"openedAt":open,"mode":"updateExisting","position":{"type":"initialZero"},
                    "baseFilesystemEpoch":0,"streamRevision":0,"content":{"type":"literal","value":"é\n","evidence":literal},
                    "encoding":"utf8","newline":"preserve","requiredResult":"fullCharacterCount"},
                "dependencies":[{"type":"capability","assumption":"standardPythonEvaluation"},{"type":"capability","assumption":"standardBuiltinOpen"},
                    {"type":"capability","assumption":"noConcurrentMutation"},{"type":"filesystemEpoch","epoch":0},{"type":"entryWorkingDirectory","cwd":"/workspace"}],
                "predecessorConditions":[open],"finalState":"opaqueSuccessor"}},
            "predecessorConditions":[],"carrierAssumptions":[],"laterContext":"opaqueSuffix",
            "snapshot":{"basis":"preExecutionSnapshotConditional","scope":"logicalStream","sourceId":0,"filesystemEpoch":0,
                "before":"old\n","after":"é\n\n","unifiedDiff":"--- supplied-before\n+++ conditional-after\n@@ -1,1 +1,2 @@\n-old\n+é\n+\n",
                "readBindings":[],"textProfile":{"readNewline":"sourceDeclared","writeNewline":"preserve","runtimeDefaultEncoding":null},
                "environmentId":"local","resolvedCwd":"/workspace","resolvedTarget":"/workspace/note.txt","replayOrdinals":[],
                "logicalStream":{"binding":{"sourceId":0,"streamId":0,"openedAt":open,"baseFilesystemEpoch":0,"revision":0},
                    "mode":"updateExisting","position":{"type":"initialZero"},"revisionAfter":1,"payloadBytes":[195,169,10],
                    "acceptedCharactersRequired":2,"realization":"requiresSuccessfulFlushOrClose"}}
        }],"barriers":[]
    })
}

fn typed(value: Value) -> SourceDescription {
    serde_json::from_value(value).expect("authored wire fixture")
}

fn rejected(value: Value) {
    assert_eq!(
        typed(value).into_presentable(|_| true),
        SourceDescription::Fallback {
            reason: SourceDescriptionFallback::Presentation
        }
    );
}

#[test]
fn logical_utf8_payload_counts_characters_and_retains_suffix() {
    let value = typed(fixture());
    assert_eq!(value.clone().into_presentable(|_| true), value);
}

#[test]
fn logical_binding_scope_count_and_seed_tampering_is_rejected() {
    for index in 0..12 {
        let mut value = fixture();
        let snapshot = &mut value["edits"][0]["snapshot"];
        match index {
            0 => snapshot["scope"] = json!("operationPoint"),
            1 => {
                snapshot
                    .as_object_mut()
                    .expect("object")
                    .remove("logicalStream");
            }
            2 => snapshot["logicalStream"]["binding"]["sourceId"] = json!(1),
            3 => snapshot["logicalStream"]["binding"]["streamId"] = json!(1),
            4 => snapshot["logicalStream"]["binding"]["baseFilesystemEpoch"] = json!(1),
            5 => snapshot["logicalStream"]["binding"]["revision"] = json!(1),
            6 => snapshot["logicalStream"]["revisionAfter"] = json!(2),
            7 => snapshot["logicalStream"]["acceptedCharactersRequired"] = json!(3),
            8 => snapshot["logicalStream"]["payloadBytes"] = json!([195]),
            9 => snapshot["before"] = Value::Null,
            10 => snapshot["logicalStream"]["mode"] = json!("truncateWrite"),
            _ => snapshot["textProfile"]["writeNewline"] = json!("lf"),
        }
        rejected(value);
    }
}

#[test]
fn logical_write_cannot_claim_current_or_future_completion() {
    let mut value = fixture();
    let current = value["edits"][0]["evidence"].clone();
    value["edits"][0]["details"]["prediction"]["predecessorConditions"]
        .as_array_mut()
        .expect("conditions")
        .push(current);
    rejected(value);
}

#[test]
fn logical_result_cannot_seed_a_later_physical_effect() {
    let mut value = fixture();
    let mut next = value["edits"][0].clone();
    next["ordinal"] = json!(1);
    next["details"]["prediction"]["ordinal"] = json!(1);
    next["details"]["prediction"]["operation"] = json!({"type":"truncate"});
    next["details"]["prediction"]["dependencies"][3]["epoch"] = json!(1);
    next.as_object_mut().expect("edit").remove("snapshot");
    value["edits"].as_array_mut().expect("edits").push(next);
    rejected(value);
}

#[test]
fn legacy_optional_fields_remain_absent_on_roundtrip() {
    let value: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/source_description_selective.json"
    )))
    .expect("legacy fixture");
    assert_eq!(
        serde_json::to_value(typed(value.clone())).expect("serialize"),
        value
    );
}

#[test]
fn logical_base_epoch_does_not_follow_event_ordinal() {
    let mut value = fixture();
    let mut prior = value["edits"][0].clone();
    prior["details"]["prediction"]["operation"] = json!({"type":"truncate"});
    prior.as_object_mut().expect("edit").remove("snapshot");
    let mut logical = value["edits"][0].clone();
    logical["ordinal"] = json!(1);
    logical["details"]["prediction"]["ordinal"] = json!(1);
    logical.as_object_mut().expect("edit").remove("snapshot");
    value["edits"] = json!([prior, logical]);
    let description = typed(value.clone());
    assert_eq!(description.clone().into_presentable(|_| true), description);
    value["edits"][1]["details"]["prediction"]["dependencies"][3]["epoch"] = json!(1);
    rejected(value);
}

#[test]
fn rewind_requires_ordered_read_and_seek_proofs_even_for_literal_payload() {
    let mut value = fixture();
    let source = "f = open('note.txt', 'r+', encoding='utf-8', newline='')\ns = f.read()\nf.seek(0)\nf.write('é\\n')\n";
    value["sources"][0]["text"] = json!(source);
    let write = source_span(source, "f.write('é\\n')");
    let read = source_span(source, "f.read()");
    let seek = source_span(source, "f.seek(0)");
    value["edits"][0]["evidence"] = write.clone();
    value["edits"][0]
        .as_object_mut()
        .expect("edit")
        .remove("snapshot");
    let prediction = &mut value["edits"][0]["details"]["prediction"];
    prediction["evidence"] = write;
    prediction["operation"]["content"]["evidence"] = source_span(source, "'é\\n'");
    prediction["operation"]["position"] =
        json!({"type":"rewoundZero","readId":0,"readEvidence":read,"seekEvidence":seek});
    prediction["predecessorConditions"]
        .as_array_mut()
        .expect("conditions")
        .extend([read.clone(), seek]);
    prediction["dependencies"]
        .as_array_mut()
        .expect("dependencies")
        .push(json!({"type":"readValue","readId":0,"evidence":read}));
    let description = typed(value.clone());
    assert_eq!(description.clone().into_presentable(|_| true), description);
    value["edits"][0]["details"]["prediction"]["predecessorConditions"]
        .as_array_mut()
        .expect("conditions")
        .swap(1, 2);
    rejected(value);
}

#[test]
fn evaluated_join_argument_may_precede_logical_write_inside_its_span() {
    let mut value = fixture();
    let source = "f = open('note.txt', 'r+', encoding='utf-8', newline='')\nf.write(''.join(['é', '\\n']))\nf.close()\n";
    let write = source_span(source, "f.write(''.join(['é', '\\n']))");
    let join = source_span(source, "''.join(['é', '\\n'])");
    value["sources"][0]["text"] = json!(source);
    value["edits"][0]["evidence"] = write.clone();
    let prediction = &mut value["edits"][0]["details"]["prediction"];
    prediction["evidence"] = write;
    prediction["operation"]["content"] = json!({"type":"concat",
        "left":{"type":"literal","value":"é","evidence":source_span(source,"'é'")},
        "right":{"type":"literal","value":"\n","evidence":source_span(source,"'\\n'")},
        "evidence":join});
    prediction["dependencies"]
        .as_array_mut()
        .expect("dependencies")
        .push(json!({"type":"capability","assumption":"standardTextMethods"}));
    prediction["predecessorConditions"]
        .as_array_mut()
        .expect("conditions")
        .push(join);
    value["edits"][0]["details"]["assumptions"]
        .as_array_mut()
        .expect("assumptions")
        .push(json!("standardTextMethods"));
    let description = typed(value.clone());
    assert_eq!(description.clone().into_presentable(|_| true), description);
    value["edits"][0]["details"]["prediction"]["predecessorConditions"]
        .as_array_mut()
        .expect("conditions")
        .push(source_span(source, "f.close()"));
    rejected(value);
}

#[test]
fn authored_spans_use_utf8_byte_columns_after_multibyte_text() {
    let span = source_span("é 'x'", "'x'");
    assert_eq!(span["startByte"], json!(3));
    assert_eq!(span["startColumn"], json!(4));
}
