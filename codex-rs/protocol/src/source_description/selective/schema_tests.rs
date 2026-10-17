use super::SelectiveJavaScriptDependency;
use super::SelectiveJavaScriptDescriptorPosition;
use super::SelectiveJavaScriptTextRecipe;
use super::SelectiveJavaScriptWriteEffect;
use super::SelectiveLogicalStreamPreviewMetadata;
use super::SelectivePredicateRecipe;
use super::SelectivePythonStreamPosition;
use super::SelectiveTarget;
use super::SelectiveTextEncoding;
use super::SelectiveTextRecipe;
use super::SourceDescriptionSpan;
use pretty_assertions::assert_eq;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;
use serde_json::json;
use std::collections::BTreeSet;
use ts_rs::TS;

fn evidence() -> SourceDescriptionSpan {
    SourceDescriptionSpan {
        source_id: 0,
        start_byte: 0,
        end_byte: 1,
        start_line: 1,
        end_line: 1,
        start_column: 1,
    }
}

fn target() -> SelectiveTarget {
    SelectiveTarget {
        path: "input.txt".to_owned(),
        cwd: Some("/workspace".to_owned()),
    }
}

fn literal() -> SelectiveTextRecipe {
    SelectiveTextRecipe::Literal {
        value: "abc".to_owned(),
        evidence: evidence(),
    }
}

fn assert_schema_matches_wire<T: Serialize + JsonSchema>(
    value: &T,
    expected: Value,
    optional_fields: &[&str],
) {
    let wire = serde_json::to_value(value).expect("serialize protocol value");
    assert_eq!(wire, expected);

    let schema = serde_json::to_value(schemars::schema_for!(T)).expect("serialize schema");
    let variant = schema["oneOf"]
        .as_array()
        .expect("tagged enum schema")
        .iter()
        .find(|variant| variant["properties"]["type"]["enum"][0] == wire["type"])
        .expect("wire discriminator has a schema variant");
    let properties = variant["properties"]
        .as_object()
        .expect("object properties");
    let wire_fields: BTreeSet<_> = wire
        .as_object()
        .expect("object wire value")
        .keys()
        .map(String::as_str)
        .collect();
    let schema_fields: BTreeSet<_> = properties.keys().map(String::as_str).collect();
    assert_eq!(schema_fields, wire_fields);
    assert_eq!(variant["additionalProperties"], json!(false));

    let required: BTreeSet<_> = variant["required"]
        .as_array()
        .expect("required fields")
        .iter()
        .map(|field| field.as_str().expect("required field name"))
        .collect();
    let expected_required: BTreeSet<_> = wire_fields
        .into_iter()
        .filter(|field| !optional_fields.contains(field))
        .collect();
    assert_eq!(required, expected_required);
}

#[test]
fn python_read_schema_matches_camel_case_wire_fields() {
    let value = SelectiveTextRecipe::Read {
        newline: Some(super::SelectivePythonReadNewline::Preserve),
        read_id: 7,
        target: target(),
        evidence: evidence(),
        filesystem_epoch: 2,
        encoding: SelectiveTextEncoding::Utf8,
    };
    assert_schema_matches_wire(
        &value,
        json!({
            "type": "read", "readId": 7, "target": target(), "evidence": evidence(),
            "filesystemEpoch": 2, "encoding": "utf8", "newline":"preserve"
        }),
        &["newline"],
    );
}

#[test]
fn python_split_item_schema_matches_camel_case_wire_fields() {
    let value = SelectiveTextRecipe::SplitItem {
        input: Box::new(literal()),
        separator: ",".to_owned(),
        maxsplit: Some(2),
        index: -1,
        split_evidence: evidence(),
        evidence: evidence(),
    };
    assert_schema_matches_wire(
        &value,
        json!({
            "type": "splitItem", "input": literal(), "separator": ",", "maxsplit": 2,
            "index": -1, "splitEvidence": evidence(), "evidence": evidence()
        }),
        &["maxsplit"],
    );
}

#[test]
fn python_starts_with_schema_matches_camel_case_wire_fields() {
    let value = SelectivePredicateRecipe::StartsWith {
        input: Box::new(literal()),
        prefix: "a".to_owned(),
        prefix_evidence: evidence(),
        evidence: evidence(),
    };
    assert_schema_matches_wire(
        &value,
        json!({
            "type": "startsWith", "input": literal(), "prefix": "a",
            "prefixEvidence": evidence(), "evidence": evidence()
        }),
        &[],
    );
}

#[test]
fn python_ends_with_schema_matches_camel_case_wire_fields() {
    let value = SelectivePredicateRecipe::EndsWith {
        input: Box::new(literal()),
        suffix: "c".to_owned(),
        suffix_evidence: evidence(),
        evidence: evidence(),
    };
    assert_schema_matches_wire(
        &value,
        json!({
            "type": "endsWith", "input": literal(), "suffix": "c",
            "suffixEvidence": evidence(), "evidence": evidence()
        }),
        &[],
    );
}

#[test]
fn javascript_read_utf8_schema_matches_camel_case_wire_fields() {
    let value = SelectiveJavaScriptTextRecipe::ReadUtf8 {
        read_id: 7,
        target: target(),
        evidence: evidence(),
        filesystem_epoch: 2,
    };
    assert_schema_matches_wire(
        &value,
        json!({
            "type": "readUtf8", "readId": 7, "target": target(),
            "evidence": evidence(), "filesystemEpoch": 2
        }),
        &[],
    );
}

#[test]
fn javascript_read_value_schema_matches_camel_case_wire_fields() {
    let value = SelectiveJavaScriptDependency::ReadValue {
        read_id: 7,
        evidence: evidence(),
    };
    assert_schema_matches_wire(
        &value,
        json!({"type": "readValue", "readId": 7, "evidence": evidence()}),
        &[],
    );
}

fn assert_typescript_field_names<T: TS>(fields: &[(&str, &str)]) {
    let declaration = T::decl();
    for (wire, rust) in fields {
        assert!(declaration.contains(&format!("{wire}:")), "{declaration}");
        assert!(!declaration.contains(&format!("{rust}:")), "{declaration}");
    }
}

#[test]
fn javascript_truncate_schema_and_typescript_match_wire_names() {
    let value = SelectiveJavaScriptWriteEffect::Truncate {
        descriptor_id: 7,
        opened_at: evidence(),
    };
    assert_schema_matches_wire(
        &value,
        json!({"type":"truncate", "descriptorId":7, "openedAt":evidence()}),
        &[],
    );
    assert_typescript_field_names::<SelectiveJavaScriptWriteEffect>(&[
        ("descriptorId", "descriptor_id"),
        ("openedAt", "opened_at"),
    ]);
}

#[test]
fn javascript_descriptor_write_schema_and_typescript_match_wire_names() {
    let value = SelectiveJavaScriptWriteEffect::DescriptorWrite {
        descriptor_id: 7,
        opened_at: evidence(),
        byte_offset: 2,
        position: SelectiveJavaScriptDescriptorPosition::Absolute,
    };
    assert_schema_matches_wire(
        &value,
        json!({
            "type":"descriptorWrite", "descriptorId":7, "openedAt":evidence(),
            "byteOffset":2, "position":"absolute"
        }),
        &[],
    );
    assert_typescript_field_names::<SelectiveJavaScriptWriteEffect>(&[
        ("descriptorId", "descriptor_id"),
        ("openedAt", "opened_at"),
        ("byteOffset", "byte_offset"),
    ]);
}

#[test]
fn javascript_descriptor_dependency_schema_and_typescript_match_wire_names() {
    let value = SelectiveJavaScriptDependency::Descriptor {
        descriptor_id: 7,
        opened_at: evidence(),
    };
    assert_schema_matches_wire(
        &value,
        json!({"type":"descriptor", "descriptorId":7, "openedAt":evidence()}),
        &[],
    );
    assert_typescript_field_names::<SelectiveJavaScriptDependency>(&[
        ("descriptorId", "descriptor_id"),
        ("openedAt", "opened_at"),
    ]);
}

#[test]
fn javascript_slice_schema_preserves_utf16_bounds_and_nullable_end() {
    let input = SelectiveJavaScriptTextRecipe::Literal {
        value: "🦉tail".to_owned(),
        evidence: evidence(),
    };
    for end in [None, Some(2)] {
        let value = SelectiveJavaScriptTextRecipe::Slice {
            input: Box::new(input.clone()),
            start: 0,
            end,
            evidence: evidence(),
        };
        assert_schema_matches_wire(
            &value,
            json!({"type":"slice","input":input,"start":0,"end":end,"evidence":evidence()}),
            &["end"],
        );
    }
    let declaration = SelectiveJavaScriptTextRecipe::decl();
    assert!(declaration.contains("start: number"), "{declaration}");
    assert!(declaration.contains("end: number | null"), "{declaration}");
}

#[test]
fn javascript_descriptor_append_schema_and_typescript_match_wire_names() {
    let value = SelectiveJavaScriptWriteEffect::DescriptorAppend {
        descriptor_id: 7,
        opened_at: evidence(),
    };
    assert_schema_matches_wire(
        &value,
        json!({"type":"descriptorAppend", "descriptorId":7, "openedAt":evidence()}),
        &[],
    );
    assert_typescript_field_names::<SelectiveJavaScriptWriteEffect>(&[
        ("descriptorId", "descriptor_id"),
        ("openedAt", "opened_at"),
    ]);
}

#[test]
fn python_rewound_position_schema_and_typescript_match_wire_names() {
    let read_evidence = evidence();
    let seek_evidence = SourceDescriptionSpan {
        start_byte: 2,
        end_byte: 3,
        start_column: 3,
        ..evidence()
    };
    let value = SelectivePythonStreamPosition::RewoundZero {
        read_id: 7,
        read_evidence: read_evidence.clone(),
        seek_evidence: seek_evidence.clone(),
    };
    let expected = json!({
        "type": "rewoundZero", "readId": 7,
        "readEvidence": read_evidence, "seekEvidence": seek_evidence
    });
    assert_schema_matches_wire(&value, expected.clone(), &[]);
    assert_eq!(
        serde_json::from_value::<SelectivePythonStreamPosition>(expected)
            .expect("deserialize camelCase rewind proof"),
        value
    );
    assert_typescript_field_names::<SelectivePythonStreamPosition>(&[
        ("readId", "read_id"),
        ("readEvidence", "read_evidence"),
        ("seekEvidence", "seek_evidence"),
    ]);
}

#[test]
fn logical_payload_schema_has_explicit_byte_item_bounds() {
    let schema = serde_json::to_value(schemars::schema_for!(SelectiveLogicalStreamPreviewMetadata))
        .expect("serialize logical metadata schema");
    let payload = &schema["properties"]["payloadBytes"];
    assert_eq!(payload["type"], json!("array"));
    assert_eq!(payload["items"]["type"], json!("integer"));
    assert_eq!(payload["items"]["minimum"], json!(0.0));
    assert_eq!(payload["items"]["maximum"], json!(255.0));
}
