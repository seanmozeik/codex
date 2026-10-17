//! Malformed logical bindings never trigger a physical-view retry.

use super::capture;
use codex_code_activity::SnapshotReadNewline;
use codex_code_activity::SnapshotTextProfile;
use codex_code_activity::SnapshotWriteNewline;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn request() -> Value {
    let physical = json!({
        "ordinal": 1, "sourceId": 0, "filesystemEpoch": 1,
        "path": "out", "cwd": null, "bytes": [],
    });
    let mut invalid_logical = physical.clone();
    invalid_logical["logicalStream"] = json!({
        "sourceId": 999, "streamId": 0, "baseFilesystemEpoch": 1, "revision": 0,
        "openedAt": {"sourceId": 0, "startByte": 5, "endByte": 46,
            "startLine": 0, "endLine": 0, "startColumn": 5},
    });
    json!({
        "id": "endpoint-kind", "language": "python", "cwd": null,
        "snapshotCallId": "endpoint-kind",
        "source": "with open('out','w',encoding='utf8',newline='') as f: f.write('X')",
        "textProfile": SnapshotTextProfile::new(
            SnapshotReadNewline::SourceDeclared, SnapshotWriteNewline::Preserve),
        "targets": [physical, invalid_logical],
    })
}

#[test]
fn mixed_invalid_logical_binding_keeps_logical_view_and_refuses_physical_replay() -> TestResult {
    let result: Value = serde_json::from_slice(&capture(&request().to_string())?)?;
    assert_eq!(result["construction"], "accepted");
    assert_eq!(
        result["analysis"]["edits"][1]["details"]["prediction"]["operation"]["type"],
        "logicalWrite"
    );
    assert_eq!(result["previews"][0]["disposition"], "refused");
    assert_eq!(
        result["previews"][0]["reason"]["type"],
        "unsupportedOperation"
    );
    assert_eq!(result["previews"][1]["disposition"], "invalidBinding");
    Ok(())
}

#[test]
fn explicit_close_view_does_not_turn_a_logical_binding_into_physical_authority() -> TestResult {
    let mut input = request();
    input["pythonWriteEndpoint"] = json!("contextCloseWhenAvailable");
    let result: Value = serde_json::from_slice(&capture(&input.to_string())?)?;
    assert_eq!(result["construction"], "accepted");
    assert_eq!(
        result["analysis"]["edits"][1]["details"]["prediction"]["operation"]["type"],
        "writeText"
    );
    assert_eq!(result["previews"][0]["disposition"], "preview");
    assert_eq!(result["previews"][0]["preview"]["afterBytes"], json!([88]));
    assert_eq!(result["previews"][1]["disposition"], "invalidBinding");
    Ok(())
}
