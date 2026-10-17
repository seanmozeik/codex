//! Authored transport payloads test structural validation, never semantic authority.

use crate::source_description::SourceDescription;
use crate::source_description::SourceDescriptionFallback;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

fn span(source: &str, needle: &str) -> Value {
    let start = source.find(needle).expect("authored span");
    let end = start + needle.len();
    json!({"sourceId":0,"startByte":start,"endByte":end,"startLine":source[..start].bytes().filter(|byte| *byte == b'\n').count()+1,"endLine":source[..end].bytes().filter(|byte| *byte == b'\n').count()+1,"startColumn":source[..start].rsplit('\n').next().expect("prefix").len()+1})
}

fn fixture(new: &str) -> Value {
    let literal = serde_json::to_string(new).expect("source string token");
    let expression = format!("text.replaceAll('old', {literal})");
    let write = format!("write('out', {expression}, 'utf8')");
    let source = format!(
        "const {{readFileSync:read, writeFileSync:write}}=require('fs');\nconst text=read('in','utf8');\n{write};\n"
    );
    let read = span(&source, "read('in','utf8')");
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
    json!({"disposition":"selectiveFindings","sources":[{"id":0,"language":"typeScript","parent":null,"text":source}],"edits":[{
        "ordinal":0,"basis":"standardApiForecast","evidence":operation,
        "details":{"kind":"javaScript","assumptions":assumptions,"prediction":{
            "ordinal":0,"evidence":operation,"target":{"path":"out","cwd":"/workspace"},
            "content":{"type":"replaceAll","input":{"type":"readUtf8","readId":0,"target":{"path":"in","cwd":"/workspace"},"evidence":read,"filesystemEpoch":0},"old":"old","new":new,"evidence":span(&source,&expression)},
            "dependencies":dependencies,"predecessorConditions":[span(&source,"require('fs')"),read]
        }},"predecessorConditions":[],"carrierAssumptions":[],"laterContext":"unobserved"
    }],"barriers":[]})
}

fn descriptor_fixture() -> Value {
    let source =
        "const fs=require('fs'); const fd=fs.openSync('out','r+'); fs.writeSync(fd,'x',2,'utf8');";
    let opening = span(source, "fs.openSync('out','r+')");
    let operation = span(source, "fs.writeSync(fd,'x',2,'utf8')");
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
            "writeEffect":{"type":"descriptorWrite","descriptorId":0,"openedAt":opening,"byteOffset":2,"position":"absolute"},
            "dependencies":dependencies,"predecessorConditions":[opening]
        }},"predecessorConditions":[],"carrierAssumptions":[],"laterContext":"unobserved"
    }],"barriers":[]})
}

fn content(value: &mut Value) -> &mut Value {
    &mut value["edits"][0]["details"]["prediction"]["content"]
}

fn rejected(value: Value) {
    let description: SourceDescription = serde_json::from_value(value).expect("typed DTO");
    assert_eq!(
        description.into_presentable(|_| true),
        SourceDescription::Fallback {
            reason: SourceDescriptionFallback::Presentation,
        }
    );
}

#[test]
fn all_match_recipe_and_safe_newline_text_round_trip_without_normalization() {
    for replacement in ["new", "\n", "😀\n界"] {
        let value = fixture(replacement);
        let description: SourceDescription =
            serde_json::from_value(value.clone()).expect("typed DTO");
        assert_eq!(description.clone().into_presentable(|_| true), description);
        assert_eq!(
            serde_json::to_value(description).expect("round trip"),
            value
        );
    }
}

#[test]
fn empty_needle_substitutions_control_text_and_wrong_newline_coordinates_refuse() {
    for change in 0..9 {
        let mut value = fixture("new");
        match change {
            0 => content(&mut value)["old"] = json!(""),
            1 => content(&mut value)["new"] = json!("$&"),
            2 => content(&mut value)["new"] = json!("\r\n"),
            3 => content(&mut value)["old"] = json!("\u{1b}[31m"),
            4 => content(&mut value)["evidence"]["startLine"] = json!(2),
            5 => content(&mut value)["evidence"]["endLine"] = json!(4),
            6 => content(&mut value)["input"]["evidence"]["startColumn"] = json!(1),
            7 => content(&mut value)["input"]["readId"] = json!(99),
            _ => content(&mut value)["input"]["filesystemEpoch"] = json!(1),
        }
        rejected(value);
    }
}

#[test]
fn replacement_arguments_respect_disclosure_and_required_capabilities() {
    for secret in ["old", "new"] {
        let input: SourceDescription = serde_json::from_value(fixture("new")).expect("typed DTO");
        assert_eq!(
            input.into_presentable(|text| text != secret),
            SourceDescription::Fallback {
                reason: SourceDescriptionFallback::Presentation,
            }
        );
    }
    let mut value = fixture("new");
    value["edits"][0]["details"]["prediction"]["dependencies"]
        .as_array_mut()
        .expect("dependencies")
        .retain(|dependency| dependency["assumption"] != "standardStringMethods");
    rejected(value);
}

#[test]
fn malformed_all_match_payload_fields_are_rejected_at_decode() {
    for change in 0..4 {
        let mut value = fixture("new");
        match change {
            0 => content(&mut value)["count"] = json!(1),
            1 => content(&mut value)["old"] = json!(["old"]),
            2 => {
                content(&mut value)
                    .as_object_mut()
                    .expect("recipe")
                    .remove("input");
            }
            _ => content(&mut value)["applied"] = json!(true),
        }
        assert!(serde_json::from_value::<SourceDescription>(value).is_err());
    }
}

#[test]
fn descriptor_effect_round_trips_with_explicit_file_and_completion_premises() {
    let value = descriptor_fixture();
    let description: SourceDescription =
        serde_json::from_value(value.clone()).expect("descriptor DTO");
    assert_eq!(description.clone().into_presentable(|_| true), description);
    assert_eq!(
        serde_json::to_value(description).expect("round trip"),
        value
    );
}

#[test]
fn descriptor_effect_rejects_inconsistent_capability_offset_and_opening() {
    for change in 0..8 {
        if change == 3 && usize::BITS <= 53 {
            continue;
        }
        let mut value = descriptor_fixture();
        let details = &mut value["edits"][0]["details"];
        match change {
            0 => details["assumptions"]
                .as_array_mut()
                .expect("assumptions")
                .retain(|value| value != "regularDescriptorFiles"),
            1 => details["prediction"]["dependencies"]
                .as_array_mut()
                .expect("dependencies")
                .retain(|value| value["assumption"] != "completeDescriptorWrites"),
            2 => details["prediction"]["writeEffect"]["descriptorId"] = json!(1),
            3 => {
                details["prediction"]["writeEffect"]["byteOffset"] =
                    json!(9_007_199_254_740_992_u64)
            }
            4 => details["prediction"]["writeEffect"]["openedAt"]["sourceId"] = json!(1),
            5 => {
                details["prediction"]["writeEffect"]["openedAt"] =
                    details["prediction"]["evidence"].clone()
            }
            6 => details["prediction"]["predecessorConditions"] = json!([]),
            _ => details["prediction"]["writeEffect"]["type"] = json!("truncate"),
        }
        if change == 7 {
            let effect = details["prediction"]["writeEffect"]
                .as_object_mut()
                .expect("effect");
            effect.remove("byteOffset");
            effect.remove("position");
        }
        rejected(value);
    }
}

fn slice_fixture(end: Option<usize>) -> Value {
    let arguments = end.map_or_else(|| "0".to_owned(), |end| format!("0,{end}"));
    let expression = format!("text.slice({arguments})");
    let write = format!("fs.writeFileSync('out',{expression})");
    let source =
        format!("const fs=require('fs'); const text=fs.readFileSync('in','utf8'); {write};");
    let read = span(&source, "fs.readFileSync('in','utf8')");
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
    json!({"disposition":"selectiveFindings","sources":[{"id":0,"language":"typeScript","parent":null,"text":source}],"edits":[{
        "ordinal":0,"basis":"standardApiForecast","evidence":operation,
        "details":{"kind":"javaScript","assumptions":assumptions,"prediction":{
            "ordinal":0,"evidence":operation,"target":{"path":"out","cwd":"/workspace"},
            "content":{"type":"slice","input":{"type":"readUtf8","readId":0,"target":{"path":"in","cwd":"/workspace"},"evidence":read,"filesystemEpoch":0},"start":0,"end":end,"evidence":span(&source,&expression)},
            "dependencies":dependencies,"predecessorConditions":[span(&source,"require('fs')"),read]
        }},"predecessorConditions":[],"carrierAssumptions":[],"laterContext":"unobserved"
    }],"barriers":[]})
}

#[test]
fn utf16_slice_round_trips_with_captured_read_and_optional_end() {
    for end in [None, Some(2)] {
        let value = slice_fixture(end);
        let description: SourceDescription =
            serde_json::from_value(value.clone()).expect("slice DTO");
        assert_eq!(description.clone().into_presentable(|_| true), description);
        assert_eq!(
            serde_json::to_value(description).expect("round trip"),
            value
        );
    }
}

#[test]
fn slice_refuses_inconsistent_capabilities_read_identity_and_evidence() {
    for change in 0..5 {
        let mut value = slice_fixture(Some(2));
        match change {
            0 => value["edits"][0]["details"]["prediction"]["dependencies"]
                .as_array_mut()
                .expect("dependencies")
                .retain(|value| value["assumption"] != "standardStringMethods"),
            1 => content(&mut value)["input"]["readId"] = json!(9),
            2 => content(&mut value)["input"]["filesystemEpoch"] = json!(1),
            3 => content(&mut value)["evidence"]["sourceId"] = json!(1),
            _ => content(&mut value)["evidence"]["startLine"] = json!(2),
        }
        rejected(value);
    }
}

#[test]
fn slice_bounds_reject_coercion_unknown_fields_and_unsafe_integers() {
    for (field, invalid) in [
        ("start", json!(-1)),
        ("start", json!(1.5)),
        ("start", json!("0")),
        ("start", Value::Null),
        ("end", json!(-1)),
        ("end", json!("2")),
        ("step", json!(1)),
        ("applied", json!(true)),
    ] {
        let mut value = slice_fixture(Some(2));
        content(&mut value)[field] = invalid;
        assert!(
            serde_json::from_value::<SourceDescription>(value).is_err(),
            "{field}"
        );
    }
    if usize::BITS > 53 {
        for field in ["start", "end"] {
            let mut value = slice_fixture(Some(2));
            content(&mut value)[field] = json!(9_007_199_254_740_992_u64);
            rejected(value);
        }
    }
}
