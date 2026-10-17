use super::support::TestResult;
use super::support::analyze;
use super::support::input;
use codex_code_activity::LogicalStreamRealization;
use codex_code_activity::PythonStreamId;
use codex_code_activity::SelectiveEditDetails;
use codex_code_activity::SnapshotEpoch;
use codex_code_activity::SnapshotPreviewError;
use codex_code_activity::SnapshotPreviewScope;
use codex_code_activity::materialize_logical_stream_preview;
use codex_code_activity::materialize_snapshot_preview;
use pretty_assertions::assert_eq;

#[test]
fn logical_update_preserves_suffix_and_counts_characters_without_physical_replay() -> TestResult {
    let source = "with open('out', 'r+', encoding='utf8', newline='') as f:\n    f.write('é')";
    let analysis = analyze(source)?;
    assert!(analysis.python_barriers().is_empty());
    assert_eq!(analysis.edits().len(), 1);
    let input = input(&analysis, 0, Some(b"abcd"))?;
    let preview = materialize_logical_stream_preview(&analysis, 0, &input)?;
    assert_eq!(preview.scope, SnapshotPreviewScope::LogicalStream);
    assert_eq!(preview.after_bytes, "écd".as_bytes());
    let logical = preview.logical_stream.ok_or("missing logical metadata")?;
    assert_eq!(logical.payload_bytes, "é".as_bytes());
    assert_eq!(logical.accepted_characters_required, 1);
    assert_eq!(logical.revision_after, 1);
    assert_eq!(
        logical.realization,
        LogicalStreamRealization::RequiresSuccessfulFlushOrClose
    );
    assert!(matches!(
        materialize_snapshot_preview(&analysis, 0, &input.snapshot),
        Err(SnapshotPreviewError::UnsupportedOperation)
    ));
    let SelectiveEditDetails::Python { prediction, .. } = &analysis.edits()[0].details else {
        return Err("not Python".into());
    };
    assert!(
        !prediction
            .predecessor_conditions
            .contains(&prediction.evidence)
    );
    assert_eq!(prediction.predecessor_conditions.len(), 1);
    Ok(())
}

#[test]
fn logical_truncation_requires_empty_present_seed_and_exact_open_revision() -> TestResult {
    let analysis = analyze(
        "with open('out', 'w', encoding='utf8', newline='') as f:\n    f.write('new')\n    f.close()",
    )?;
    assert_eq!(analysis.edits().len(), 2);
    let valid = input(&analysis, 1, Some(b""))?;
    let preview = materialize_logical_stream_preview(&analysis, 1, &valid)?;
    assert_eq!(preview.filesystem_epoch, SnapshotEpoch::new(1));
    assert_eq!(preview.after_bytes, b"new");
    for field in 0..4 {
        let mut wrong = valid.clone();
        match field {
            0 => wrong.binding.revision = 1,
            1 => wrong.binding.stream_id = PythonStreamId::new(1),
            2 => wrong.binding.opened_at.end_byte += 1,
            _ => wrong.binding.base_filesystem_epoch = SnapshotEpoch::new(0),
        }
        assert!(matches!(
            materialize_logical_stream_preview(&analysis, 1, &wrong),
            Err(SnapshotPreviewError::LogicalStreamBindingMismatch)
        ));
    }
    assert!(matches!(
        materialize_logical_stream_preview(&analysis, 1, &input(&analysis, 1, None)?),
        Err(SnapshotPreviewError::MissingLogicalStreamPreimage)
    ));
    assert!(matches!(
        materialize_logical_stream_preview(&analysis, 1, &input(&analysis, 1, Some(b"old"))?),
        Err(SnapshotPreviewError::LogicalStreamSeedMismatch)
    ));
    assert!(matches!(
        materialize_logical_stream_preview(&analysis, 0, &valid),
        Err(SnapshotPreviewError::UnsupportedOperation)
    ));
    Ok(())
}
