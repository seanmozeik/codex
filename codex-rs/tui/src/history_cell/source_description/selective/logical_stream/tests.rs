//! Authored DTOs exercise presentation and restoration without executing source.

mod rewound;
mod tamper;

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

fn span(source: &str, needle: &str) -> Value {
    let start = source.find(needle).expect("authored span");
    let end = start + needle.len();
    json!({
        "sourceId":0,"startByte":start,"endByte":end,
        "startLine":source[..start].bytes().filter(|byte| *byte == b'\n').count()+1,
        "endLine":source[..end].bytes().filter(|byte| *byte == b'\n').count()+1,
        "startColumn":source[..start].rsplit('\n').next().expect("prefix").len()+1
    })
}

fn fixture() -> Value {
    let source =
        "with open('note.txt', 'r+', encoding='utf-8', newline='') as f:\n    f.write('é\\n')\n";
    let opened = span(
        source,
        "open('note.txt', 'r+', encoding='utf-8', newline='')",
    );
    let write = span(source, "f.write('é\\n')");
    let position = json!({"type":"initialZero"});
    let assumptions = [
        "standardPythonEvaluation",
        "standardBuiltinOpen",
        "noConcurrentMutation",
    ];
    let mut dependencies = assumptions
        .iter()
        .map(|assumption| json!({"type":"capability","assumption":assumption}))
        .collect::<Vec<_>>();
    dependencies.extend([
        json!({"type":"filesystemEpoch","epoch":0}),
        json!({"type":"entryWorkingDirectory","cwd":"/workspace"}),
    ]);
    json!({
        "disposition":"selectiveFindings",
        "sources":[{"id":0,"language":"python","parent":null,"text":source}],
        "edits":[{
            "ordinal":0,"basis":"standardApiForecast","evidence":write,
            "details":{"kind":"python","assumptions":assumptions,"prediction":{
                "ordinal":0,"evidence":write,"target":{"path":"note.txt","cwd":"/workspace"},
                "operation":{
                    "type":"logicalWrite","streamId":0,"openedAt":opened,
                    "mode":"updateExisting","position":position,
                    "baseFilesystemEpoch":0,"streamRevision":0,
                    "content":{"type":"literal","value":"é\n","evidence":span(source,"'é\\n'")},
                    "encoding":"utf8","newline":"preserve","requiredResult":"fullCharacterCount"
                },
                "dependencies":dependencies,"predecessorConditions":[opened],
                "finalState":"notEstablished"
            }},
            "predecessorConditions":[],"carrierAssumptions":[],"laterContext":"unobserved",
            "snapshot":{
                "basis":"preExecutionSnapshotConditional","scope":"logicalStream",
                "sourceId":0,"filesystemEpoch":0,"before":"xx\n","after":"é\n",
                "unifiedDiff":"--- supplied-before\n+++ conditional-after\n@@ -1,1 +1,1 @@\n-xx\n+é\n",
                "readBindings":[],
                "textProfile":{"readNewline":"sourceDeclared","writeNewline":"preserve","runtimeDefaultEncoding":null},
                "environmentId":"local","resolvedCwd":"/workspace",
                "resolvedTarget":"/workspace/note.txt","replayOrdinals":[],
                "logicalStream":{
                    "binding":{"sourceId":0,"streamId":0,"openedAt":opened,"baseFilesystemEpoch":0,"revision":0},
                    "mode":"updateExisting","position":position,"revisionAfter":1,
                    "payloadBytes":[195,169,10],"acceptedCharactersRequired":2,
                    "realization":"requiresSuccessfulFlushOrClose"
                }
            }
        }],"barriers":[]
    })
}

fn plain(lines: Vec<Line<'static>>) -> String {
    lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

fn restore(description: SourceDescription) -> crate::thread_transcript::TranscriptCells {
    let item = ThreadItem::SourceDescription {
        id: "logical".to_owned(),
        call_id: "call".to_owned(),
        description,
    };
    let wire = serde_json::to_vec(&item).expect("serialize authored item");
    let item = serde_json::from_slice(&wire).expect("restore authored item");
    crate::thread_transcript::thread_items_to_transcript_cells(
        None,
        &test_path_buf("/workspace").abs(),
        vec![item],
        crate::thread_transcript::RawReasoningVisibility::Hidden,
        None,
    )
}

#[test]
fn logical_diff_conditions_precede_bytes_in_live_and_restored_views() {
    let input: SourceDescription = serde_json::from_value(fixture()).expect("authored logical DTO");
    let live =
        SourceDescriptionCell::new("logical".to_owned(), input.clone()).expect("logical cell");
    let restored = restore(input);
    assert_eq!(restored.len(), 1);
    for cell in [&live as &dyn HistoryCell, restored[0].as_ref()] {
        let compact = plain(cell.display_lines(240));
        for expected in [
            "Conditional logical stream diff · before execution",
            "Requires full character count: 2 accepted before newline translation; 3 encoded payload bytes.",
            "Physical realization requires a later successful flush or close.",
            "current and final file bytes are unobserved.",
        ] {
            assert!(compact.contains(expected), "missing {expected}: {compact}");
            assert!(
                compact.find(expected).expect("condition") < compact.find("-xx").expect("diff")
            );
        }
        let detail = plain(cell.raw_lines());
        for expected in [
            "conditional logical stream write",
            "Stream 0:0 · opened at source 0:1:6 · revision 0 · physical seed epoch 0",
            "Mode: r+ · retain the untouched encoded-byte suffix",
            "Position: initial zero",
            "output newline preserve characters",
            "Conditional logical stream diff from a physical seed",
        ] {
            assert!(detail.contains(expected), "missing {expected}: {detail}");
        }
        for forbidden in [
            "Applied",
            "Succeeded",
            "Observed diff",
            "operation point",
            "final bytes:",
        ] {
            assert!(!compact.contains(forbidden), "unexpected {forbidden}");
            assert!(!detail.contains(forbidden), "unexpected {forbidden}");
        }
    }
    for width in [24, 80, 160] {
        assert_eq!(live.display_lines(width), restored[0].display_lines(width));
        assert_eq!(
            live.transcript_lines(width),
            restored[0].transcript_lines(width)
        );
        assert!(
            restored[0]
                .transcript_hyperlink_lines(width)
                .iter()
                .all(|line| line.hyperlinks.is_empty())
        );
    }
}

#[test]
fn logical_no_op_keeps_acceptance_and_future_realization_conditions() {
    let mut value = fixture();
    value["edits"][0]["snapshot"]["before"] = json!("é\n");
    value["edits"][0]["snapshot"]["unifiedDiff"] = json!("");
    let input: SourceDescription = serde_json::from_value(value).expect("authored no-op DTO");
    let live =
        SourceDescriptionCell::new("logical".to_owned(), input.clone()).expect("logical no-op");
    let restored = restore(input);
    assert_eq!(restored.len(), 1);
    for cell in [&live as &dyn HistoryCell, restored[0].as_ref()] {
        let compact = plain(cell.display_lines(240));
        for expected in [
            "Conditional logical stream diff",
            "Requires full character count: 2",
            "later successful flush or close",
            "No byte change in this conditional logical stream.",
        ] {
            assert!(compact.contains(expected), "missing {expected}: {compact}");
        }
        assert!(!compact.contains("No byte change at this operation point."));
    }
}

#[test]
fn source_only_logical_summary_retains_newline_and_realization_contract() {
    let mut value = fixture();
    value["edits"][0]
        .as_object_mut()
        .expect("edit")
        .remove("snapshot");
    let input: SourceDescription = serde_json::from_value(value).expect("source-only DTO");
    let live =
        SourceDescriptionCell::new("logical".to_owned(), input.clone()).expect("source-only cell");
    let restored = restore(input);
    assert_eq!(restored.len(), 1);
    for cell in [&live as &dyn HistoryCell, restored[0].as_ref()] {
        let compact = plain(cell.display_lines(240));
        for expected in [
            "conditional logical stream",
            "output newline preserve characters",
            "requires full character count and later successful flush or close",
        ] {
            assert!(compact.contains(expected), "missing {expected}: {compact}");
        }
        assert!(!compact.contains("Conditional logical stream diff"));
    }
}
