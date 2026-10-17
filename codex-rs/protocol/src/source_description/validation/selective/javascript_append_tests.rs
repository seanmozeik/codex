//! Append transport validation is structural; these payloads do not execute Node.

use crate::source_description::SourceDescription;
use crate::source_description::SourceDescriptionFallback;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

fn span(source: &str, needle: &str) -> Value {
    let start = source.find(needle).expect("authored span");
    json!({"sourceId":0,"startByte":start,"endByte":start+needle.len(),"startLine":1,"endLine":1,"startColumn":start+1})
}

fn fixture() -> Value {
    let source = "const fs=require('fs'); const fd=fs.openSync('out','a'); fs.writeSync(fd,'x');";
    let opening = span(source, "fs.openSync('out','a')");
    let operation = span(source, "fs.writeSync(fd,'x')");
    let assumptions = [
        "standardNodeCommonJsEvaluation",
        "canonicalNodeFs",
        "noConcurrentMutation",
        "regularDescriptorFiles",
        "completeDescriptorWrites",
    ];
    let mut dependencies = assumptions
        .iter()
        .map(|assumption| json!({"type":"capability","assumption":assumption}))
        .collect::<Vec<_>>();
    dependencies.extend([
        json!({"type":"entryWorkingDirectory","cwd":"/workspace"}),
        json!({"type":"filesystemEpoch","epoch":0}),
        json!({"type":"descriptor","descriptorId":0,"openedAt":opening}),
    ]);
    json!({"disposition":"selectiveFindings","sources":[{"id":0,"language":"typeScript","parent":null,"text":source}],"edits":[{
        "ordinal":0,"basis":"standardApiForecast","evidence":operation,
        "details":{"kind":"javaScript","assumptions":assumptions,"prediction":{
            "ordinal":0,"evidence":operation,"target":{"path":"out","cwd":"/workspace"},
            "content":{"type":"literal","value":"x","evidence":span(source,"'x'")},
            "writeEffect":{"type":"descriptorAppend","descriptorId":0,"openedAt":opening},
            "dependencies":dependencies,"predecessorConditions":[opening]
        }},"predecessorConditions":[],"carrierAssumptions":[],"laterContext":"unobserved"
    }],"barriers":[]})
}

fn rejected(value: Value) {
    let description: SourceDescription = serde_json::from_value(value).expect("typed append DTO");
    assert_eq!(
        description.into_presentable(|_| true),
        SourceDescription::Fallback {
            reason: SourceDescriptionFallback::Presentation,
        }
    );
}

#[test]
fn append_effect_round_trips_with_existing_descriptor_premises() {
    let value = fixture();
    let description: SourceDescription = serde_json::from_value(value.clone()).expect("append DTO");
    assert_eq!(description.clone().into_presentable(|_| true), description);
    assert_eq!(
        serde_json::to_value(description).expect("round trip"),
        value
    );
}

#[test]
fn append_requires_each_assumption_and_each_capability() {
    for assumption in ["regularDescriptorFiles", "completeDescriptorWrites"] {
        for remove_assumption in [false, true] {
            let mut value = fixture();
            let details = &mut value["edits"][0]["details"];
            if remove_assumption {
                details["assumptions"]
                    .as_array_mut()
                    .expect("assumptions")
                    .retain(|entry| entry != assumption);
            } else {
                details["prediction"]["dependencies"]
                    .as_array_mut()
                    .expect("dependencies")
                    .retain(|entry| entry["assumption"] != assumption);
            }
            rejected(value);
        }
    }
}

#[test]
fn append_rejects_corrupt_descriptor_identity_opening_and_order() {
    for change in 0..8 {
        let mut value = fixture();
        let prediction = &mut value["edits"][0]["details"]["prediction"];
        match change {
            0 => prediction["writeEffect"]["descriptorId"] = json!(1),
            1 => prediction["writeEffect"]["openedAt"]["sourceId"] = json!(1),
            2 => prediction["writeEffect"]["openedAt"] = prediction["evidence"].clone(),
            3 => prediction["predecessorConditions"] = json!([]),
            4 => prediction["writeEffect"]["openedAt"]["endByte"] = json!(9999),
            5 => prediction["writeEffect"]["openedAt"]["startColumn"] = json!(1),
            6 => {
                prediction["dependencies"]
                    .as_array_mut()
                    .expect("dependencies")
                    .retain(|entry| entry["type"] != "descriptor");
            }
            _ => {
                prediction["writeEffect"]["descriptorId"] = json!(usize::MAX);
                for entry in prediction["dependencies"]
                    .as_array_mut()
                    .expect("dependencies")
                {
                    if entry["type"] == "descriptor" {
                        entry["descriptorId"] = json!(usize::MAX);
                    }
                }
            }
        }
        rejected(value);
    }
    // Keep the effect and predecessor internally consistent while corrupting only
    // the dependency's opening, so identity cannot be checked by id alone.
    let mut value = fixture();
    let other = span(
        value["sources"][0]["text"].as_str().expect("source"),
        "require('fs')",
    );
    let prediction = &mut value["edits"][0]["details"]["prediction"];
    for entry in prediction["dependencies"]
        .as_array_mut()
        .expect("dependencies")
    {
        if entry["type"] == "descriptor" {
            entry["openedAt"] = other.clone();
        }
    }
    rejected(value);
}

#[test]
fn append_decode_rejects_extra_offset_position_missing_fields_and_null() {
    for (field, invalid) in [
        ("byteOffset", json!(0)),
        ("position", json!("cursor")),
        ("applied", json!(true)),
        ("descriptorId", json!(-1)),
        ("descriptorId", json!("0")),
        ("openedAt", Value::Null),
        ("type", json!("unknownAppend")),
    ] {
        let mut value = fixture();
        value["edits"][0]["details"]["prediction"]["writeEffect"][field] = invalid;
        assert!(
            serde_json::from_value::<SourceDescription>(value).is_err(),
            "{field}"
        );
    }
    for field in ["descriptorId", "openedAt", "type"] {
        let mut value = fixture();
        value["edits"][0]["details"]["prediction"]["writeEffect"]
            .as_object_mut()
            .expect("effect")
            .remove(field);
        assert!(
            serde_json::from_value::<SourceDescription>(value).is_err(),
            "{field}"
        );
    }
    let mut value = fixture();
    value["edits"][0]["details"]["prediction"]["writeEffect"] = Value::Null;
    assert!(serde_json::from_value::<SourceDescription>(value).is_err());
}
