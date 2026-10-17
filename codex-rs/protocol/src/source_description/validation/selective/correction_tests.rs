//! V2 closed-payload corrections. These are authored DTOs, not executed source.

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
    .expect("shared authored fixture")
}

fn accepted(value: Value) {
    let input: SourceDescription = serde_json::from_value(value).expect("typed fixture");
    assert_eq!(input.clone().into_presentable(|_| true), input);
}

fn rejected(value: Value) {
    let input: SourceDescription = serde_json::from_value(value).expect("typed fixture");
    assert_eq!(
        input.into_presentable(|_| true),
        SourceDescription::Fallback {
            reason: SourceDescriptionFallback::Presentation,
        }
    );
}

fn span(source: &str, needle: &str) -> Value {
    let start = source.find(needle).expect("authored source span");
    let end = start + needle.len();
    json!({
        "sourceId":0, "startByte":start, "endByte":end,
        "startLine":source[..start].bytes().filter(|byte| *byte == b'\n').count() + 1,
        "endLine":source[..end].bytes().filter(|byte| *byte == b'\n').count() + 1,
        "startColumn":source[..start].rsplit('\n').next().expect("prefix").len() + 1,
    })
}

fn open_pair(captured_read: bool) -> Value {
    let prefix = if captured_read {
        "s = open('in.txt').read()\n"
    } else {
        ""
    };
    let call = if captured_read {
        "open('out.txt', 'w').write(s)"
    } else {
        "open('out.txt', 'w').write('new')"
    };
    let source = format!("{prefix}{call}\n");
    let open = span(&source, "open('out.txt', 'w')");
    let write = span(&source, call);
    let cwd = json!("/workspace");
    let assumptions = json!([
        "standardPythonEvaluation",
        "standardBuiltinOpen",
        "standardTextMethods",
        "noConcurrentMutation"
    ]);
    let common = vec![
        json!({"type":"capability","assumption":"standardPythonEvaluation"}),
        json!({"type":"capability","assumption":"standardBuiltinOpen"}),
        json!({"type":"capability","assumption":"noConcurrentMutation"}),
        json!({"type":"entryWorkingDirectory","cwd":cwd}),
    ];
    let mut truncate_dependencies = common.clone();
    truncate_dependencies.push(json!({"type":"filesystemEpoch","epoch":0}));
    let mut write_dependencies = common;
    write_dependencies.push(json!({"type":"filesystemEpoch","epoch":1}));
    let mut truncate_conditions = Vec::new();
    let mut write_conditions = vec![open.clone()];
    let content = if captured_read {
        let read = span(&source, "open('in.txt').read()");
        truncate_conditions.push(read.clone());
        write_conditions.insert(0, read.clone());
        write_dependencies.push(json!({"type":"readValue","readId":0,"evidence":read}));
        write_dependencies.push(json!({"type":"filesystemEpoch","epoch":0}));
        json!({
            "type":"read","readId":0, "target":{"path":"in.txt","cwd":cwd},
            "evidence":read, "filesystemEpoch":0, "encoding":"runtimeDefault",
        })
    } else {
        json!({"type":"literal","value":"new","evidence":span(&source, "'new'")})
    };
    let target = json!({"path":"out.txt","cwd":cwd});
    json!({
        "disposition":"selectiveFindings",
        "sources":[{"id":0,"language":"python","parent":null,"text":source}],
        "edits":[
            {
                "ordinal":0,"basis":"standardApiForecast","evidence":open,
                "details":{"kind":"python","assumptions":assumptions,"prediction":{
                    "ordinal":0,"evidence":open,"target":target,
                    "operation":{"type":"truncate"},"dependencies":truncate_dependencies,
                    "predecessorConditions":truncate_conditions,"finalState":"knownLaterTouch"
                }},
                "predecessorConditions":[],"carrierAssumptions":[],"laterContext":"knownLaterTouch"
            },
            {
                "ordinal":1,"basis":"standardApiForecast","evidence":write,
                "details":{"kind":"python","assumptions":assumptions,"prediction":{
                    "ordinal":1,"evidence":write,"target":target,
                    "operation":{"type":"writeText","content":content,"encoding":"runtimeDefault"},
                    "dependencies":write_dependencies,"predecessorConditions":write_conditions,
                    "finalState":"notEstablished"
                }},
                "predecessorConditions":[],"carrierAssumptions":[],"laterContext":"unobserved"
            }
        ],
        "barriers":[],
    })
}

fn remove_capability(value: &mut Value, edit: usize, capability: &str, declaration: bool) {
    let details = &mut value["edits"][edit]["details"];
    details["prediction"]["dependencies"]
        .as_array_mut()
        .expect("dependencies")
        .retain(|dependency| {
            dependency["type"] != "capability" || dependency["assumption"] != capability
        });
    if declaration {
        details["assumptions"]
            .as_array_mut()
            .expect("assumptions")
            .retain(|assumption| assumption != capability);
    }
}

#[test]
fn nested_python_requires_exact_ordered_dispatch_and_version_pairs() {
    accepted(fixture());
    for change in 0..8 {
        let mut value = fixture();
        let carriers = value["edits"][1]["carrierAssumptions"]
            .as_array_mut()
            .expect("carriers");
        match change {
            0 => carriers.clear(),
            1 => {
                carriers.remove(2);
            }
            2 => {
                carriers.remove(1);
            }
            3 => carriers.swap(1, 2),
            4 => carriers.push(carriers[2].clone()),
            5 => carriers[2]["language"] = json!("shell"),
            6 => carriers[2]["invocation"] = carriers[0]["invocation"].clone(),
            _ => {
                carriers.remove(0);
            }
        }
        rejected(value);
    }
    let mut value = fixture();
    value["edits"][0]["carrierAssumptions"] = value["edits"][1]["carrierAssumptions"].clone();
    rejected(value);
}

#[test]
fn carrier_invocation_must_own_the_direct_parent_interval() {
    let mut value = fixture();
    let body = value["sources"][2]["parent"].clone();
    let header_text = value["sources"][1]["text"].as_str().expect("shell source");
    let mut header = span(header_text, "python3");
    header["sourceId"] = json!(1);
    value["edits"][1]["carrierAssumptions"][1]["invocation"] = header.clone();
    value["edits"][1]["carrierAssumptions"][2]["invocation"] = header;
    assert_ne!(
        body["startByte"],
        value["edits"][1]["carrierAssumptions"][1]["invocation"]["startByte"]
    );
    rejected(value);
}

#[test]
fn minimum_operation_capabilities_cannot_be_removed_with_their_declarations() {
    accepted(open_pair(false));
    let mut value = open_pair(false);
    remove_capability(&mut value, 0, "standardBuiltinOpen", true);
    rejected(value);
    let mut value = fixture();
    remove_capability(&mut value, 1, "canonicalPathlib", true);
    rejected(value);
    for capability in ["standardPythonEvaluation", "noConcurrentMutation"] {
        let mut value = open_pair(false);
        remove_capability(&mut value, 1, capability, false);
        rejected(value);
    }
    let mut value = open_pair(false);
    value["edits"][0]["details"]["prediction"]["dependencies"] = json!([]);
    rejected(value);
}

#[test]
fn text_operations_require_their_declared_text_capability() {
    let mut value = fixture();
    remove_capability(&mut value, 1, "standardTextMethods", true);
    rejected(value);
    let mut value = open_pair(false);
    let original = value["edits"][1]["details"]["prediction"]["operation"]["content"].clone();
    value["edits"][1]["details"]["prediction"]["operation"]["content"] = json!({
        "type":"concat","left":original,"right":original,"evidence":original["evidence"],
    });
    rejected(value.clone());
    value["edits"][1]["details"]["prediction"]["dependencies"]
        .as_array_mut()
        .expect("dependencies")
        .push(json!({"type":"capability","assumption":"standardTextMethods"}));
    accepted(value);
}

#[test]
fn current_and_captured_epochs_coexist_but_neither_can_be_falsified_or_omitted() {
    let mut value = open_pair(true);
    // Extra bounded source-local provenance need not occur as a content leaf.
    let evidence = value["edits"][0]["evidence"].clone();
    value["edits"][1]["details"]["prediction"]["dependencies"]
        .as_array_mut()
        .expect("dependencies")
        .push(json!({"type":"readValue","readId":7,"evidence":evidence}));
    accepted(value);
    for epoch in [0, 1] {
        let mut value = open_pair(true);
        value["edits"][1]["details"]["prediction"]["dependencies"]
            .as_array_mut()
            .expect("dependencies")
            .retain(|dependency| {
                dependency["type"] != "filesystemEpoch" || dependency["epoch"] != epoch
            });
        rejected(value);
    }
    let mut value = open_pair(true);
    value["edits"][1]["details"]["prediction"]["dependencies"]
        .as_array_mut()
        .expect("dependencies")
        .push(json!({"type":"filesystemEpoch","epoch":2}));
    rejected(value);
}

#[test]
fn target_cwd_requires_a_matching_dependency_and_refuses_contradiction() {
    for contradiction in [false, true] {
        let mut value = open_pair(false);
        let dependencies = value["edits"][1]["details"]["prediction"]["dependencies"]
            .as_array_mut()
            .expect("dependencies");
        if contradiction {
            dependencies.push(json!({"type":"entryWorkingDirectory","cwd":"/elsewhere"}));
        } else {
            dependencies.retain(|dependency| dependency["type"] != "entryWorkingDirectory");
        }
        rejected(value);
    }
    let mut value = open_pair(false);
    value["edits"][1]["details"]["prediction"]["dependencies"]
        .as_array_mut()
        .expect("dependencies")
        .push(json!({"type":"entryWorkingDirectory","cwd":"/workspace"}));
    accepted(value);
}

#[test]
fn prerequisite_ranges_admit_nested_receivers_and_reject_successors_or_other_sources() {
    accepted(open_pair(false)); // The open receiver is inside the write invocation.
    accepted(open_pair(true)); // A captured read before the current write remains valid.
    let mut value = fixture();
    value["edits"][1]["predecessorConditions"][0] =
        value["edits"][1]["carrierAssumptions"][0]["invocation"].clone();
    rejected(value);
    let mut value = fixture();
    value["edits"][1]["details"]["prediction"]["predecessorConditions"][0] =
        value["barriers"][0]["evidence"].clone();
    rejected(value);
    let mut value = fixture();
    value["edits"][1]["details"]["prediction"]["predecessorConditions"][0] =
        value["edits"][1]["evidence"].clone();
    rejected(value);
    let mut value = fixture();
    let mut sibling = value["sources"][2].clone();
    sibling["id"] = json!(3);
    value["sources"]
        .as_array_mut()
        .expect("sources")
        .push(sibling);
    value["edits"][1]["predecessorConditions"][0] = value["barriers"][0]["evidence"].clone();
    value["edits"][1]["predecessorConditions"][0]["sourceId"] = json!(3);
    rejected(value);
}

#[test]
fn carrier_reason_categories_are_closed() {
    let mut value = fixture();
    let evidence = value["edits"][0]["evidence"].clone();
    for reason in [
        "unknownExecution",
        "unsupportedControl",
        "unresolvedDispatch",
        "unresolvedArguments",
        "contextMutation",
        "concurrentExecution",
        "unresolvedPatch",
        "unresolvedChild",
    ] {
        value["barriers"]
            .as_array_mut()
            .expect("barriers")
            .push(json!({"evidence":evidence,"reason":{"type":"carrier","reason":reason}}));
    }
    accepted(value);
    let mut value = fixture();
    value["barriers"][0]["reason"] = json!({"type":"carrier","reason":"alreadyApplied"});
    rejected(value);
}

#[test]
fn python_sources_cannot_claim_to_launch_owned_python_children() {
    fn remap_evidence(value: &mut Value, source_id: u64) {
        match value {
            Value::Object(fields) => {
                if let Some(id) = fields.get_mut("sourceId") {
                    *id = json!(source_id);
                }
                for child in fields.values_mut() {
                    remap_evidence(child, source_id);
                }
            }
            Value::Array(items) => {
                for item in items {
                    remap_evidence(item, source_id);
                }
            }
            _ => {}
        }
    }
    let value = fixture();
    let mut root = value["sources"][2].clone();
    root["id"] = json!(0);
    root["parent"] = Value::Null;
    let mut parent =
        value["edits"][1]["details"]["prediction"]["operation"]["content"]["input"]["evidence"]
            .clone();
    parent["sourceId"] = json!(0);
    let mut child = root.clone();
    child["id"] = json!(1);
    child["parent"] = parent.clone();
    let mut edit = value["edits"][1].clone();
    edit["ordinal"] = json!(0);
    edit["predecessorConditions"] = json!([]);
    edit["carrierAssumptions"] = json!([]);
    remap_evidence(&mut edit, 1);
    edit["carrierAssumptions"] = json!([
        {"basis":"standardShellDispatchAndStartup","language":"python","invocation":parent},
        {"basis":"standardPython3Cli","language":"python","invocation":parent},
    ]);
    rejected(json!({
        "disposition":"selectiveFindings","sources":[root,child],"edits":[edit],"barriers":[],
    }));
}
