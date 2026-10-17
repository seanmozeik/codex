//! The full consumer pipeline parses authored Node and never executes it.

use super::Fixture;
use super::code_mode;
use super::convert;
use super::source_description_for_display;
use codex_protocol::source_description::SelectiveEditDetails;
use codex_protocol::source_description::SelectiveJavaScriptDependency;
use codex_protocol::source_description::SelectiveJavaScriptWriteEffect;
use codex_protocol::source_description::SourceDescription;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn append_replay_preserves_old_read_and_survives_display_without_disk_writes() {
    let fixture = Fixture::new().await;
    fixture.write("file", "界old".as_bytes()).await;
    let analysis = code_mode(
        "const fs=require('fs'); const saved=fs.readFileSync('file','utf8'); const fd=fs.openSync('file','a'); fs.writeSync(fd,'!'); fs.writeSync(fd,saved); fs.closeSync(fd);",
        "node",
    );
    assert_eq!(analysis.edits().len(), 2);
    let previews = fixture.supply(&analysis).await;
    assert_eq!(previews.len(), 2);
    assert_eq!(previews[1].preview.read_bindings.len(), 1);
    assert_eq!(
        previews[1].preview.read_bindings[0].filesystem_epoch.get(),
        0
    );
    let description = convert::forecast(&analysis, &previews);
    let serialized = serde_json::to_value(&description).expect("append wire serialization");
    let decoded = serde_json::from_value(serialized).expect("append wire round trip");
    assert_eq!(source_description_for_display(decoded), description);
    let SourceDescription::SelectiveFindings { edits, .. } = description else {
        panic!("append forecast must survive conversion");
    };
    assert_eq!(edits.len(), 2);
    for (index, edit) in edits.iter().enumerate() {
        let SelectiveEditDetails::JavaScript { prediction, .. } = &edit.details else {
            panic!("Node prediction");
        };
        let SelectiveJavaScriptWriteEffect::DescriptorAppend {
            descriptor_id,
            opened_at,
        } = &prediction.write_effect
        else {
            panic!("EOF-relative effect");
        };
        assert_eq!(*descriptor_id, 0);
        assert_eq!(opened_at.source_id, edit.evidence.source_id);
        assert!(prediction.predecessor_conditions.contains(opened_at));
        assert!(prediction.dependencies.iter().any(|dependency| {
            matches!(dependency, SelectiveJavaScriptDependency::Descriptor { descriptor_id: id, opened_at: opening } if id == descriptor_id && opening == opened_at)
        }));
        let snapshot = edit.snapshot.as_ref().expect("conditional append snapshot");
        assert_eq!(snapshot.source_id, edit.evidence.source_id);
        if index == 0 {
            assert_eq!(snapshot.before.as_deref(), Some("界old"));
            assert_eq!(snapshot.after, "界old!");
            assert!(snapshot.replay_ordinals.is_empty());
        } else {
            assert_eq!(snapshot.before.as_deref(), Some("界old!"));
            assert_eq!(snapshot.after, "界old!界old");
            assert_eq!(snapshot.replay_ordinals, [0]);
            assert!(snapshot.unified_diff.contains("-界old!\n"));
            assert!(snapshot.unified_diff.contains("+界old!界old\n"));
        }
    }
    assert_eq!(fixture.bytes("file").await, "界old".as_bytes());
}

#[tokio::test]
async fn absent_append_target_stays_symbolic_without_inferred_creation() {
    let fixture = Fixture::new().await;
    let analysis = code_mode(
        "const fs=require('fs'); const fd=fs.openSync('file','a'); fs.writeSync(fd,'x'); fs.closeSync(fd);",
        "node",
    );
    assert_eq!(analysis.edits().len(), 1);
    let previews = fixture.supply(&analysis).await;
    assert!(previews.is_empty());
    let description = convert::forecast(&analysis, &previews);
    assert_eq!(
        source_description_for_display(description.clone()),
        description
    );
    let SourceDescription::SelectiveFindings { edits, .. } = description else {
        panic!("valid symbolic append remains presentable");
    };
    assert_eq!(edits.len(), 1);
    assert!(edits[0].snapshot.is_none());
    assert!(
        !fixture
            ._directory
            .path()
            .join("file")
            .try_exists()
            .expect("fixture metadata")
    );
}
