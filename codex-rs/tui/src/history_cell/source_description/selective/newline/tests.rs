//! Authored policy DTOs test disclosure, not source parsing or runtime behavior.

use crate::history_cell::HistoryCell;
use crate::history_cell::SourceDescriptionCell;
use crate::test_support::PathBufExt;
use crate::test_support::test_path_buf;
use codex_app_server_protocol::ThreadItem;
use codex_protocol::source_description::SourceDescription;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

#[test]
fn explicit_source_newline_policies_and_pathlib_premise_survive_restoration() {
    for (read, write, read_label, write_label) in [
        (
            "universalTranslate",
            "runtimeDefault",
            "translate universal newlines to LF",
            "runtime default (host separator required)",
        ),
        (
            "preserve",
            "preserve",
            "preserve decoded characters",
            "preserve characters",
        ),
        (
            "preserve",
            "cr",
            "preserve decoded characters",
            "translate LF to CR",
        ),
        (
            "preserve",
            "crLf",
            "preserve decoded characters",
            "translate LF to CRLF",
        ),
    ] {
        let mut value: Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../protocol/tests/fixtures/source_description_selective.json"
        )))
        .expect("authored presentation fixture");
        let details = &mut value["edits"][1]["details"];
        details["prediction"]["operation"]["newline"] = json!(write);
        details["prediction"]["operation"]["content"]["input"]["newline"] = json!(read);
        details["assumptions"]
            .as_array_mut()
            .expect("assumptions")
            .push(json!("pathlibNewlineSupport"));
        details["prediction"]["dependencies"]
            .as_array_mut()
            .expect("dependencies")
            .push(json!({"type":"capability","assumption":"pathlibNewlineSupport"}));
        let description: SourceDescription =
            serde_json::from_value(value).expect("typed policy DTO");
        let live = SourceDescriptionCell::new("newline".to_owned(), description.clone())
            .expect("policy disclosure");
        let item = ThreadItem::SourceDescription {
            id: "newline".to_owned(),
            call_id: "call".to_owned(),
            description,
        };
        let encoded = serde_json::to_vec(&item).expect("serialized policy DTO");
        let restored = crate::thread_transcript::thread_items_to_transcript_cells(
            None,
            &test_path_buf("/workspace").abs(),
            vec![serde_json::from_slice(&encoded).expect("restored policy DTO")],
            crate::thread_transcript::RawReasoningVisibility::Hidden,
            None,
        );
        assert_eq!(restored.len(), 1);
        for cell in [&live as &dyn HistoryCell, restored[0].as_ref()] {
            let raw = cell
                .raw_lines()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n");
            for expected in [
                format!("Source read newline: {read_label}"),
                format!("Source output newline: {write_label}"),
                "pathlib newline API support (read_text: Python 3.13+; write_text: Python 3.10+)"
                    .to_owned(),
            ] {
                assert!(raw.contains(&expected), "missing {expected}: {raw}");
            }
            let compact = cell
                .display_lines(240)
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n");
            assert!(compact.contains(&format!("output newline {write_label}")));
        }
        for width in [24, 80, 160] {
            assert_eq!(live.display_lines(width), restored[0].display_lines(width));
            assert_eq!(
                live.transcript_lines(width),
                restored[0].transcript_lines(width)
            );
        }
    }
}
