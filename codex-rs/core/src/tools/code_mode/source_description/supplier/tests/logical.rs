//! Stream projections use disposable supplied bytes; represented Python is inert.

use super::Fixture;
use super::SnapshotSupplier;
use super::activity;
use super::convert;
use super::python;
use codex_protocol::source_description as wire;
use pretty_assertions::assert_eq;

const INITIAL: &str =
    "with open('file', 'r+', encoding='utf-8', newline='') as stream:\n    stream.write('done')";

#[tokio::test]
async fn logical_noop_is_supplied_and_keeps_its_source_write_anchor() {
    let fixture = Fixture::new().await;
    fixture.write("file", b"done\n").await;
    let analysis = python(INITIAL);
    let previews = fixture.supply(&analysis).await;
    assert_eq!(previews.len(), 1);
    let preview = &previews[0].preview;
    assert_eq!(preview.scope, activity::SnapshotPreviewScope::LogicalStream);
    assert_eq!(preview.after_bytes, b"done\n");
    assert!(preview.unified_diff.is_empty());
    let metadata = preview.logical_stream.as_ref().expect("logical proof");
    assert_eq!(metadata.accepted_characters_required, 4);
    assert_eq!(metadata.payload_bytes, b"done");
    assert_eq!(metadata.binding.base_filesystem_epoch.get(), 0);
    assert_eq!(metadata.binding.revision, 0);
    assert_eq!(metadata.revision_after, 1);
    assert_eq!(
        INITIAL.get(preview.evidence.start_byte..preview.evidence.end_byte),
        Some("stream.write('done')")
    );
    assert!(
        preview
            .predecessor_conditions
            .iter()
            .all(|span| { span.end_byte <= preview.evidence.start_byte })
    );
    let wire::SourceDescription::SelectiveFindings { edits, .. } =
        convert::forecast(&analysis, &previews)
    else {
        panic!("logical source is displayable");
    };
    let snapshot = edits[0]
        .snapshot
        .as_ref()
        .expect("UI receives logical diff");
    assert_eq!(snapshot.scope, wire::SelectiveSnapshotScope::LogicalStream);
    assert!(snapshot.logical_stream.is_some());
    assert_eq!(fixture.bytes("file").await, b"done\n");
}

#[tokio::test]
async fn logical_rewind_preserves_original_read_epoch_and_untouched_suffix() {
    let fixture = Fixture::new().await;
    fixture.write("file", b"go tail\n").await;
    let analysis = python(
        "with open('file', 'r+', encoding='utf-8', newline='') as stream:\n    saved = stream.read()\n    stream.seek(0)\n    stream.write(saved.replace('go', 'X', 1))",
    );
    let previews = fixture.supply(&analysis).await;
    assert_eq!(previews.len(), 1);
    let preview = &previews[0].preview;
    assert_eq!(preview.after_bytes, b"X tail\n\n");
    assert_eq!(preview.read_bindings.len(), 1);
    assert_eq!(preview.read_bindings[0].filesystem_epoch.get(), 0);
    assert_eq!(
        preview.read_bindings[0].newline,
        activity::SnapshotReadNewline::Preserve
    );
    let metadata = preview.logical_stream.as_ref().expect("logical proof");
    assert!(matches!(
        metadata.position,
        activity::PythonStreamPosition::RewoundZero { .. }
    ));
    assert_eq!(metadata.accepted_characters_required, 7);
    assert_eq!(fixture.bytes("file").await, b"go tail\n");
}

#[tokio::test]
async fn logical_write_uses_empty_replayed_seed_and_old_captured_read() {
    let fixture = Fixture::new().await;
    fixture.write("species", b"oak\n").await;
    fixture.write("file", b"old card\n").await;
    let analysis = python(
        "with open('species', 'r', encoding='utf-8', newline='') as reader:\n    saved = reader.read()\nwith open('file', 'w', encoding='utf-8', newline='') as writer:\n    writer.write('<' + saved + '>\\n')",
    );
    let previews = fixture.supply(&analysis).await;
    assert_eq!(previews.len(), 2);
    assert_eq!(
        previews[0].preview.scope,
        activity::SnapshotPreviewScope::OperationPoint
    );
    let preview = &previews[1].preview;
    assert_eq!(preview.scope, activity::SnapshotPreviewScope::LogicalStream);
    assert_eq!(preview.filesystem_epoch.get(), 1);
    assert_eq!(preview.before_bytes.as_deref(), Some(b"".as_slice()));
    assert_eq!(preview.after_bytes, b"<oak\n>\n");
    assert_eq!(preview.read_bindings[0].filesystem_epoch.get(), 0);
    assert_eq!(previews[1].replay_ordinals, [0]);
    let wire::SourceDescription::SelectiveFindings { edits, .. } =
        convert::forecast(&analysis, &previews)
    else {
        panic!("logical and physical source are displayable");
    };
    assert!(edits.iter().all(|edit| edit.snapshot.is_some()));
    assert_eq!(fixture.bytes("file").await, b"old card\n");
}

#[tokio::test]
async fn logical_bytes_never_supply_later_physical_or_entry_reads() {
    let fixture = Fixture::new().await;
    fixture.write("file", b"original").await;
    let analysis = python(INITIAL);
    let previews = fixture.supply(&analysis).await;
    assert_eq!(previews.len(), 1);
    let preview = &previews[0].preview;
    let mut supplier = SnapshotSupplier::new(fixture.environment.clone());
    for epoch in [0, 1] {
        assert!(
            supplier
                .point(
                    preview.evidence.source_id,
                    epoch,
                    &preview.target,
                    &previews
                )
                .await
                .is_none()
        );
    }
    assert_eq!(fixture.bytes("file").await, b"original");
}

#[tokio::test]
async fn explicit_none_and_omission_keep_equal_bytes_and_distinct_boundaries() {
    let fixture = Fixture::new().await;
    fixture.write("file", b"entry").await;
    let legacy =
        python("with open('file', 'w', encoding='utf-8') as stream:\n    stream.write('new')");
    let logical = python(
        "with open('file', 'w', encoding='utf-8', newline=None) as stream:\n    stream.write('new')",
    );
    let old = fixture.supply(&legacy).await;
    let new = fixture.supply(&logical).await;
    assert_eq!(old.len(), 2);
    assert_eq!(new.len(), 2);
    assert_eq!(old[1].preview.after_bytes, new[1].preview.after_bytes);
    assert_eq!(
        old[1].preview.scope,
        activity::SnapshotPreviewScope::OperationPoint
    );
    assert!(old[1].preview.logical_stream.is_none());
    assert_eq!(
        new[1].preview.scope,
        activity::SnapshotPreviewScope::LogicalStream
    );
    assert!(new[1].preview.logical_stream.is_some());
    assert_eq!(old[1].preview.evidence.start_byte, 0);
    assert!(new[1].preview.evidence.start_byte > 0);
}

#[tokio::test]
async fn mixed_source_newlines_survive_automatic_read_binding() {
    let fixture = Fixture::new().await;
    fixture.write("a", b"A\r\n").await;
    fixture.write("b", b"B\n").await;
    fixture.write("file", b"old").await;
    let analysis = python(
        "with open('a', 'r', encoding='utf-8', newline=None) as first:\n    a = first.read()\nwith open('b', 'r', encoding='utf-8', newline='') as second:\n    b = second.read()\nwith open('file', 'w', encoding='utf-8', newline='') as writer:\n    writer.write(a + b)",
    );
    let previews = fixture.supply(&analysis).await;
    assert_eq!(previews.len(), 2);
    let preview = &previews[1].preview;
    assert_eq!(preview.after_bytes, b"A\nB\n");
    assert_eq!(
        preview.text_profile.read_newline,
        activity::SnapshotReadNewline::SourceDeclared
    );
    assert_eq!(
        preview
            .read_bindings
            .iter()
            .map(|value| value.newline)
            .collect::<Vec<_>>(),
        [
            activity::SnapshotReadNewline::PythonUniversal,
            activity::SnapshotReadNewline::Preserve
        ]
    );
    assert_eq!(fixture.bytes("a").await, b"A\r\n");
}

#[tokio::test]
async fn absent_or_split_utf8_seed_keeps_logical_effect_symbolic() {
    let fixture = Fixture::new().await;
    let analysis = python(
        "with open('file', 'r+', encoding='utf-8', newline='') as stream:\n    stream.write('é')",
    );
    assert_eq!(analysis.edits().len(), 1);
    assert!(fixture.supply(&analysis).await.is_empty());
    fixture.write("file", "€X".as_bytes()).await;
    assert!(fixture.supply(&analysis).await.is_empty());
    let wire::SourceDescription::SelectiveFindings { edits, .. } =
        convert::forecast(&analysis, &[])
    else {
        panic!("source remains available without invalid text artifact");
    };
    assert!(edits[0].snapshot.is_none());
    assert_eq!(fixture.bytes("file").await, "€X".as_bytes());
}

#[tokio::test]
async fn later_io_is_not_forecast_from_buffered_logical_content() {
    let fixture = Fixture::new().await;
    fixture.write("file", b"old").await;
    let analysis = python(
        "with open('file', 'r+', encoding='utf-8', newline='') as stream:\n    stream.write('new')\nfrom pathlib import Path\nPath('file').write_text('later', encoding='utf-8')",
    );
    assert_eq!(analysis.edits().len(), 1);
    assert!(!analysis.python_barriers().is_empty());
    let previews = fixture.supply(&analysis).await;
    assert_eq!(previews.len(), 1);
    assert_eq!(
        previews[0].preview.scope,
        activity::SnapshotPreviewScope::LogicalStream
    );
    assert_eq!(fixture.bytes("file").await, b"old");
}

#[tokio::test]
async fn explicit_output_policy_does_not_require_an_inferred_runtime_os() {
    let mut fixture = Fixture::new().await;
    fixture.environment.executor_platform_os = Some("unknown-platform".to_owned());
    fixture.write("file", b"old").await;
    let explicit = python(
        "with open('file', 'w', encoding='utf-8', newline='') as stream:\n    stream.write('new\\n')",
    );
    let previews = fixture.supply(&explicit).await;
    assert_eq!(previews.len(), 2);
    assert_eq!(previews[1].preview.after_bytes, b"new\n");
    assert_eq!(
        previews[1].preview.scope,
        activity::SnapshotPreviewScope::LogicalStream
    );
    let runtime = python(
        "with open('file', 'w', encoding='utf-8', newline=None) as stream:\n    stream.write('new\\n')",
    );
    let previews = fixture.supply(&runtime).await;
    assert_eq!(previews.len(), 1);
    assert_eq!(previews[0].preview.after_bytes, b"");
    assert_eq!(
        previews[0].preview.scope,
        activity::SnapshotPreviewScope::OperationPoint
    );
    assert_eq!(fixture.bytes("file").await, b"old");
}
