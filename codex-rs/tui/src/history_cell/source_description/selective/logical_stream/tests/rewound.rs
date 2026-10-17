//! Rewind provenance is separate from current write acceptance and future close.

use super::fixture;
use super::plain;
use super::restore;
use super::span;
use crate::history_cell::HistoryCell;
use crate::history_cell::SourceDescriptionCell;
use codex_protocol::source_description::SourceDescription;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

fn rewound_fixture() -> Value {
    let mut value = fixture();
    let source = "with open('note.txt', 'r+', encoding='utf-8', newline='') as f:\n    text = f.read()\n    f.seek(0)\n    f.write('é\\n')\n    f.flush()\n";
    let read = span(source, "f.read()");
    let seek = span(source, "f.seek(0)");
    let write = span(source, "f.write('é\\n')");
    let position = json!({"type":"rewoundZero","readId":0,"readEvidence":read,"seekEvidence":seek});
    value["sources"][0]["text"] = json!(source);
    let edit = &mut value["edits"][0];
    edit["evidence"] = write.clone();
    let prediction = &mut edit["details"]["prediction"];
    prediction["evidence"] = write;
    prediction["operation"]["position"] = position.clone();
    prediction["operation"]["content"]["evidence"] = span(source, "'é\\n'");
    prediction["predecessorConditions"]
        .as_array_mut()
        .expect("predecessors")
        .extend([read.clone(), seek]);
    prediction["dependencies"]
        .as_array_mut()
        .expect("dependencies")
        .push(json!({"type":"readValue","readId":0,"evidence":read}));
    edit["snapshot"]["logicalStream"]["position"] = position;
    value
}

#[test]
fn rewound_stream_discloses_read_and_seek_provenance_in_live_and_restored_views() {
    let input: SourceDescription = serde_json::from_value(rewound_fixture()).expect("rewound DTO");
    let live =
        SourceDescriptionCell::new("logical".to_owned(), input.clone()).expect("rewound cell");
    let restored = restore(input);
    assert_eq!(restored.len(), 1);
    for cell in [&live as &dyn HistoryCell, restored[0].as_ref()] {
        let raw = plain(cell.raw_lines());
        assert!(raw.contains(
            "Position: zero after full read 0:0 · source 0:2:12 · seek(0) at source 0:3:5"
        ));
        assert!(raw.contains("Logical stream 0:0 · revision 0 → 1 · physical seed epoch 0"));
        assert!(
            !raw.contains("Requires earlier Python operation to complete normally · source 0:4:5")
        );
        assert!(
            !raw.contains("Requires earlier Python operation to complete normally · source 0:5:5")
        );
    }
    for width in [24, 80, 160] {
        assert_eq!(live.display_lines(width), restored[0].display_lines(width));
        assert_eq!(
            live.transcript_lines(width),
            restored[0].transcript_lines(width)
        );
    }
}

#[test]
fn current_write_or_future_flush_cannot_become_its_own_predecessor() {
    for needle in ["f.write('é\\n')", "f.flush()"] {
        let mut value = rewound_fixture();
        let evidence = span(
            value["sources"][0]["text"].as_str().expect("source"),
            needle,
        );
        value["edits"][0]["details"]["prediction"]["predecessorConditions"]
            .as_array_mut()
            .expect("predecessors")
            .push(evidence);
        let input: SourceDescription = serde_json::from_value(value).expect("tampered predecessor");
        assert!(SourceDescriptionCell::new("logical".to_owned(), input.clone()).is_none());
        assert!(restore(input).is_empty());
    }
}

fn later_touch_fixture() -> Value {
    let mut value = rewound_fixture();
    let source = format!(
        "{}    f.truncate(3)\n",
        value["sources"][0]["text"]
            .as_str()
            .expect("authored source")
    );
    value["sources"][0]["text"] = json!(source);
    value["edits"][0]["laterContext"] = json!("knownLaterTouch");
    value["edits"][0]["details"]["prediction"]["finalState"] = json!("opaqueSuccessor");
    value["barriers"] = json!([{
        "evidence":span(&source, "f.flush()"),
        "reason":{"type":"python","reason":"unsupportedOperation"}
    }]);
    value
}

#[test]
fn known_later_touch_discloses_opaque_python_suffix_and_barrier_after_restore() {
    let input: SourceDescription =
        serde_json::from_value(later_touch_fixture()).expect("authored later-touch DTO");
    let live =
        SourceDescriptionCell::new("logical".to_owned(), input.clone()).expect("logical cell");
    let restored = restore(input);
    assert_eq!(restored.len(), 1);
    for cell in [&live as &dyn HistoryCell, restored[0].as_ref()] {
        let detail = plain(cell.raw_lines());
        for expected in [
            "Conditional forecast; execution and final file state are unobserved.",
            "Python suffix: unsupported code may change the outcome.",
            "Later context: a later modeled operation touches this path spelling.",
            "Unresolved region · unsupported operation",
            "Physical realization requires a later successful flush or close.",
        ] {
            assert!(detail.contains(expected), "missing {expected}: {detail}");
        }
        for forbidden in [
            "Applied",
            "Succeeded",
            "Observed diff",
            "final bytes:",
            "Python suffix: a later modeled operation touches this path spelling.",
        ] {
            assert!(
                !detail.contains(forbidden),
                "unexpected {forbidden}: {detail}"
            );
        }
        assert!(
            plain(cell.display_lines(240)).contains("current and final file bytes are unobserved.")
        );
    }
    for width in [24, 80, 160] {
        assert_eq!(live.display_lines(width), restored[0].display_lines(width));
        assert_eq!(
            live.transcript_lines(width),
            restored[0].transcript_lines(width)
        );
    }
}

#[test]
fn known_later_touch_does_not_authorize_physical_snapshot_replay() {
    let mut value = later_touch_fixture();
    let snapshot = value["edits"][0]["snapshot"]
        .as_object_mut()
        .expect("authored logical snapshot");
    snapshot.insert("scope".to_owned(), json!("operationPoint"));
    snapshot.remove("logicalStream");
    let input: SourceDescription = serde_json::from_value(value).expect("typed physical DTO");
    assert!(SourceDescriptionCell::new("logical".to_owned(), input.clone()).is_none());
    assert!(restore(input).is_empty());
}
