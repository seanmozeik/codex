use super::super::ActivityDisclosure;
use super::super::HistoryCell;
use super::super::SourceDescriptionCell;
use crate::test_support::PathBufExt;
use crate::test_support::test_path_buf;
use codex_app_server_protocol::ThreadItem;
use codex_protocol::source_description::SourceDescription;
use pretty_assertions::assert_eq;
use ratatui::text::Line;
use serde_json::Value;
use serde_json::json;

fn fixture() -> Value {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../protocol/tests/fixtures/source_description_selective.json"
    )))
    .expect("authored selective presentation fixture")
}

fn description() -> SourceDescription {
    serde_json::from_value(fixture()).expect("typed conditional fixture")
}

fn plain(lines: Vec<Line<'static>>) -> String {
    lines
        .into_iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn conditional_live_and_restored_cells_preserve_recipe_and_uncertainty() {
    let input = description();
    let live =
        SourceDescriptionCell::new("selective".to_owned(), input.clone()).expect("live cell");
    let restored = crate::thread_transcript::thread_items_to_transcript_cells(
        None,
        &test_path_buf("/workspace").abs(),
        vec![ThreadItem::SourceDescription {
            id: "selective".to_owned(),
            call_id: "call".to_owned(),
            description: input,
        }],
        crate::thread_transcript::RawReasoningVisibility::Hidden,
        None,
    );
    assert_eq!(restored.len(), 1);
    for cell in [&live as &dyn HistoryCell, restored[0].as_ref()] {
        assert_eq!(
            plain(cell.display_lines(160)),
            concat!(
                "• Predicted edits · 2 operations · 1 unresolved regions\n",
                "  Submitted patch · 1 file directives\n",
                "  \"note.txt\" · replace \"old\" with \"new\" (all matches)",
            )
        );
        assert_eq!(cell.activity_ids(), vec!["source:selective"]);
        assert_eq!(
            cell.activity_disclosure(160),
            Some(ActivityDisclosure::Generic)
        );
        let raw = plain(cell.raw_lines());
        for expected in [
            "Conditional forecast; execution and final file state are unobserved.",
            "Requires earlier invocation to complete normally",
            "Assume standard shell dispatch and startup for shell",
            "Assume standard shell dispatch and startup for Python",
            "Assume Python 3 CLI semantics for Python",
            "Target: \"note.txt\" · cwd \"/workspace\"",
            "Read 2:0 at source-local filesystem epoch 0",
            "Replace \"old\" with \"new\", all matches",
            "Assume no concurrent mutation",
            "Later context: a later modeled operation touches this path spelling.",
            "Later context: unsupported code may change the outcome.",
            "Unresolved region · unmodeled call",
            "*** Add File: note.txt",
        ] {
            assert!(raw.contains(expected), "missing {expected}");
        }
        for forbidden in ["Applied", "Succeeded", "Observed diff", "--- a/", "+++ b/"] {
            assert!(!raw.contains(forbidden), "unexpected {forbidden}");
        }
        for width in [24, 80, 160] {
            assert_eq!(cell.transcript_lines(width), live.transcript_lines(width));
            for lines in [
                cell.display_hyperlink_lines(width),
                cell.transcript_hyperlink_lines(width),
            ] {
                assert!(lines.iter().all(|line| line.hyperlinks.is_empty()));
            }
        }
    }
}

#[test]
fn selective_disclosure_and_malformed_dependency_omit_all_detail() {
    for malformed in [false, true] {
        let mut value = fixture();
        if malformed {
            value["edits"][1]["details"]["prediction"]["dependencies"][1]["evidence"]["sourceId"] =
                json!(99);
        } else {
            value["edits"][1]["details"]["prediction"]["operation"]["content"]["new"] =
                json!("sk-abcdefghijklmnopqrstuvwx");
        }
        let input = serde_json::from_value(value).expect("typed fixture");
        assert!(SourceDescriptionCell::new("selective".to_owned(), input).is_none());
    }
}

#[test]
fn zero_predictions_do_not_claim_zero_edits() {
    let mut value = fixture();
    value["edits"] = json!([]);
    let input = serde_json::from_value(value).expect("zero-prediction fixture");
    let cell = SourceDescriptionCell::new("selective".to_owned(), input).expect("unresolved cell");
    assert!(plain(cell.raw_lines()).contains("No supported prediction was retained; this does not establish that the source makes no edits."));
}
