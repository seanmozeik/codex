mod independence;

use crate::history_cell::HistoryCell;
use crate::history_cell::SourceDescriptionCell;
use crate::test_support::PathBufExt;
use crate::test_support::test_path_buf;
use codex_app_server_protocol::ThreadItem;
use codex_protocol::source_description::SourceDescription;
use pretty_assertions::assert_eq;
use ratatui::text::Line;
use serde_json::Value;
use serde_json::json;

fn description() -> SourceDescription {
    let mut value: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../protocol/tests/fixtures/source_description_selective.json"
    )))
    .expect("authored presentation fixture");
    let mut edit = value["edits"][1].clone();
    edit["ordinal"] = json!(0);
    edit["predecessorConditions"] = json!([]);
    edit["snapshot"] = json!({
        "basis":"preExecutionSnapshotConditional", "scope":"operationPoint",
        "sourceId":2, "filesystemEpoch":0,
        "before":"old\n", "after":"new\n",
        "unifiedDiff":"--- supplied-before\n+++ conditional-after\n@@ -1,1 +1,1 @@\n-old\n+new\n",
        "readBindings":[{
            "sourceId":2, "readId":0, "filesystemEpoch":0,
            "target":{"path":"note.txt","cwd":"/workspace"},
            "encoding":"utf8", "newline":"pythonUniversal"
        }],
        "textProfile":{"readNewline":"pythonUniversal","writeNewline":"lf","runtimeDefaultEncoding":null},
        "environmentId":"local", "resolvedCwd":"/workspace",
        "resolvedTarget":"/workspace/note.txt", "replayOrdinals":[]
    });
    value["edits"] = json!([edit]);
    serde_json::from_value(value).expect("typed snapshot fixture")
}

fn plain(lines: Vec<Line<'static>>) -> String {
    lines
        .into_iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn conditional_diff_is_visible_in_compact_and_restored_history() {
    let input = description();
    let live = SourceDescriptionCell::new("snapshot".to_owned(), input.clone())
        .expect("live conditional cell");
    let restored = crate::thread_transcript::thread_items_to_transcript_cells(
        None,
        &test_path_buf("/workspace").abs(),
        vec![ThreadItem::SourceDescription {
            id: "snapshot".to_owned(),
            call_id: "call".to_owned(),
            description: input,
        }],
        crate::thread_transcript::RawReasoningVisibility::Hidden,
        None,
    );
    assert_eq!(restored.len(), 1);
    let compact = plain(live.display_lines(120));
    for expected in ["Conditional diff · before execution", "-old", "+new"] {
        assert!(compact.contains(expected), "missing {expected}");
    }
    let detail = plain(live.raw_lines());
    assert!(detail.contains("Conditional diff from pre-execution snapshots"));
    assert!(detail.contains("execution remains unobserved"));
    for forbidden in ["Applied", "Succeeded", "Observed diff"] {
        assert!(!detail.contains(forbidden), "unexpected {forbidden}");
    }
    for width in [24, 80, 160] {
        assert_eq!(live.display_lines(width), restored[0].display_lines(width));
        assert_eq!(
            live.transcript_lines(width),
            restored[0].transcript_lines(width)
        );
        assert!(
            live.transcript_hyperlink_lines(width)
                .iter()
                .all(|line| line.hyperlinks.is_empty())
        );
    }
}

#[test]
fn malformed_or_disclosure_rejected_snapshot_has_no_history_cell() {
    for replacement in [
        "secret\u{1b}[31m",
        "sk-abcdefghijklmnopqrstuvwx",
        "wrong bytes",
    ] {
        let mut input = description();
        let SourceDescription::SelectiveFindings { edits, .. } = &mut input else {
            panic!("conditional fixture");
        };
        edits[0].snapshot.as_mut().expect("snapshot").after = replacement.to_owned();
        assert!(SourceDescriptionCell::new("snapshot".to_owned(), input).is_none());
    }
}
