mod append;
mod independence;

use super::Fixture;
use crate::tools::code_mode::source_description::convert;
use codex_app_server_protocol::source_description_for_display;
use codex_code_activity as activity;
use codex_protocol::source_description::SourceDescription;
use pretty_assertions::assert_eq;
use std::time::Duration;

fn code_mode(script: &str, interpreter: &str) -> activity::SelectiveEditAnalysis {
    let command = serde_json::to_string(&format!("{interpreter} - <<'CODE'\n{script}\nCODE\n"))
        .expect("authored command");
    let source = format!("await tools.exec_command({{cmd:{command}, login:false}});");
    let input = activity::SelectiveEditInput::from_source(
        "supplier-fixture".into(),
        activity::Language::TypeScript,
        &source,
        None,
        activity::AnalysisLimits::new(32_768, Duration::from_secs(2)),
    )
    .expect("bounded source");
    activity::Analyzer::new()
        .expect("native parser")
        .forecast_edits(input)
}

#[tokio::test]
async fn code_mode_capture_materialization_and_wire_validation_form_one_pipeline() {
    let fixture = Fixture::new().await;
    fixture.write("file", b"old\n").await;
    let analysis = code_mode(super::REPLACE, "python3");
    let previews = fixture.supply(&analysis).await;
    assert_eq!(previews.len(), 1);
    let description = convert::forecast(&analysis, &previews);
    let serialized = serde_json::to_value(&description).expect("wire serialization");
    let decoded = serde_json::from_value(serialized).expect("wire roundtrip");
    assert_eq!(source_description_for_display(decoded), description);
    let SourceDescription::SelectiveFindings { edits, .. } = description else {
        panic!("selective description");
    };
    let snapshot = edits[0].snapshot.as_ref().expect("automatic exact preview");
    assert_eq!(snapshot.before.as_deref(), Some("old\n"));
    assert_eq!(snapshot.after, "new\n");
    assert!(snapshot.unified_diff.contains("-old\n+new\n"));
    assert_eq!(snapshot.source_id, edits[0].evidence.source_id);
    assert_eq!(fixture.bytes("file").await, b"old\n");
}

#[tokio::test]
async fn join_preview_survives_capture_conversion_and_display_without_a_write()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new().await;
    fixture.write("file", b"old").await;
    let analysis = code_mode(
        "from pathlib import Path\np=Path('file')\ns=p.read_text(encoding='utf-8')\np.write_text('|'.join([s, 'tail']), encoding='utf-8')",
        "python3",
    );
    let previews = fixture.supply(&analysis).await;
    assert_eq!(previews.len(), 1);
    let description = convert::forecast(&analysis, &previews);
    let decoded: SourceDescription = serde_json::from_value(serde_json::to_value(&description)?)?;
    assert_eq!(source_description_for_display(decoded), description);
    let SourceDescription::SelectiveFindings { edits, .. } = description else {
        return Err("selective join description".into());
    };
    assert_eq!(edits.len(), 1);
    let snapshot = edits[0].snapshot.as_ref().ok_or("exact join preview")?;
    assert_eq!(snapshot.before.as_deref(), Some("old"));
    assert_eq!(snapshot.after, "old|tail");
    assert!(snapshot.unified_diff.contains("-old\n"));
    assert!(snapshot.unified_diff.contains("+old|tail\n"));
    assert_eq!(snapshot.source_id, edits[0].evidence.source_id);
    assert_eq!(fixture.bytes("file").await, b"old");
    Ok(())
}

#[tokio::test]
async fn attachment_rejects_foreign_call_evidence_target_and_epoch() {
    let fixture = Fixture::new().await;
    fixture.write("file", b"old\n").await;
    let analysis = code_mode(super::REPLACE, "python3");
    let mut previews = fixture.supply(&analysis).await;
    assert_eq!(previews.len(), 1);
    let original = convert::forecast(&analysis, &[]);
    let valid = previews[0].preview.clone();
    assert_ne!(convert::forecast(&analysis, &previews), original);

    previews[0].preview.call_id = "foreign-call".into();
    assert_eq!(convert::forecast(&analysis, &previews), original);
    previews[0].preview = valid.clone();
    previews[0].preview.evidence.start_column += 1;
    assert_eq!(convert::forecast(&analysis, &previews), original);
    previews[0].preview = valid.clone();
    previews[0].preview.target = activity::Target::Literal {
        path: "foreign-target".into(),
        cwd: None,
    };
    assert_eq!(convert::forecast(&analysis, &previews), original);
    previews[0].preview = valid;
    previews[0].preview.filesystem_epoch = activity::SnapshotEpoch::new(1);
    assert_eq!(convert::forecast(&analysis, &previews), original);
}

#[tokio::test]
async fn rejected_snapshot_display_keeps_the_valid_symbolic_description() {
    let fixture = Fixture::new().await;
    fixture.write("file", b"old\r\n").await;
    let analysis = code_mode(super::REPLACE, "python3");
    let previews = fixture.supply(&analysis).await;
    assert_eq!(
        previews.len(),
        1,
        "pure UTF-8 materialization preserves CRLF"
    );
    let original = convert::forecast(&analysis, &[]);
    let enriched = convert::forecast(&analysis, &previews);
    assert_eq!(
        enriched, original,
        "display policy must not discard useful source evidence"
    );
    assert!(matches!(
        enriched,
        SourceDescription::SelectiveFindings { .. }
    ));
}

#[tokio::test]
async fn descriptor_effects_and_snapshot_replay_survive_native_conversion() {
    let fixture = Fixture::new().await;
    fixture.write("file", b"old").await;
    let analysis = code_mode(
        "const fs = require('fs'); const fd = fs.openSync('file', 'w'); fs.writeSync(fd, 'abc'); fs.writeSync(fd, 'Z', 1, 'utf8'); fs.closeSync(fd);",
        "node",
    );
    let previews = fixture.supply(&analysis).await;
    assert_eq!(previews.len(), 3);
    let description = convert::forecast(&analysis, &previews);
    let SourceDescription::SelectiveFindings { edits, .. } = description else {
        panic!("descriptor prediction must convert");
    };
    assert_eq!(edits.len(), 3);
    assert_eq!(
        edits[2].snapshot.as_ref().expect("replayed preview").after,
        "aZc"
    );
    assert_eq!(
        edits[2]
            .snapshot
            .as_ref()
            .expect("replayed preview")
            .replay_ordinals,
        [0, 1]
    );
}
