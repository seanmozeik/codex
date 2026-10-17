//! Temporary explicit-policy buffered handles do not inherit physical previews.

use super::support::TestResult;
use super::support::analyze;
use codex_code_activity::PythonBarrierReason;
use codex_code_activity::PythonOperation;
use codex_code_activity::SelectiveEditDetails;
use codex_code_activity::SnapshotEpoch;
use codex_code_activity::SnapshotPreviewInput;
use codex_code_activity::SnapshotPreviewScope;
use codex_code_activity::SnapshotTargetBinding;
use codex_code_activity::TrustedTargetSnapshot;
use codex_code_activity::materialize_snapshot_preview;
use pretty_assertions::assert_eq;

fn assert_temporary_refused(payload: &str) -> TestResult {
    let call = format!("open('out','w',encoding='utf8',newline='').write({payload})");
    let source = format!(
        "{call}\nsaved=open('out',encoding='utf8').read()\nopen('later','w',encoding='utf8').write(saved)"
    );
    let analysis = analyze(&source)?;
    assert_eq!(analysis.edits().len(), 1);
    assert_eq!(analysis.python_barriers().len(), 1);
    let barrier = &analysis.python_barriers()[0];
    assert_eq!(barrier.reason, PythonBarrierReason::UnsupportedOperation);
    assert_eq!(
        &source[barrier.evidence.start_byte..barrier.evidence.end_byte],
        call
    );
    let edit = &analysis.edits()[0];
    let SelectiveEditDetails::Python { prediction, .. } = &edit.details else {
        return Err("expected Python truncation".into());
    };
    assert!(matches!(&prediction.operation, PythonOperation::Truncate));
    assert!(prediction.predecessor_conditions.is_empty());
    let before = TrustedTargetSnapshot::new(
        SnapshotTargetBinding::new(
            edit.evidence.source_id,
            SnapshotEpoch::new(0),
            prediction.target.clone(),
        ),
        Some(b"old".to_vec()),
    );
    let input = SnapshotPreviewInput::new(analysis.call_id().clone(), before);
    let preview = materialize_snapshot_preview(&analysis, 0, &input)?;
    assert_eq!(preview.scope, SnapshotPreviewScope::OperationPoint);
    assert!(preview.logical_stream.is_none());
    assert!(preview.after_bytes.is_empty());
    assert!(preview.read_bindings.is_empty());
    Ok(())
}

#[test]
fn explicit_newline_temporary_write_keeps_only_successful_open_truncation() -> TestResult {
    assert_temporary_refused("'X'")
}

#[test]
fn explicit_newline_temporary_write_does_not_harvest_nested_payload_effects() -> TestResult {
    assert_temporary_refused("open('nested','w',encoding='utf8').write('hidden')")
}

#[test]
fn omitted_temporary_and_explicit_pathlib_write_contracts_remain_available() -> TestResult {
    let legacy = analyze("open('out','w',encoding='utf8').write('X')")?;
    assert!(legacy.python_barriers().is_empty());
    assert_eq!(legacy.edits().len(), 2);
    let path = analyze(
        "from pathlib import Path\nPath('out').write_text('X',encoding='utf8',newline='')",
    )?;
    assert!(path.python_barriers().is_empty());
    assert_eq!(path.edits().len(), 1);
    let SelectiveEditDetails::Python { prediction, .. } = &path.edits()[0].details else {
        return Err("expected pathlib write".into());
    };
    assert!(matches!(
        &prediction.operation,
        PythonOperation::WriteText {
            newline: Some(_),
            ..
        }
    ));
    Ok(())
}
