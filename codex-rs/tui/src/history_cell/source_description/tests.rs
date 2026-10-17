use super::*;
use crate::test_support::PathBufExt;
use crate::test_support::test_path_buf;
use codex_app_server_protocol::ThreadItem;
use codex_protocol::source_description::SourceDescriptionSpan;
use pretty_assertions::assert_eq;
use serde_json::json;

fn submitted_patch() -> SourceDescription {
    let payload = "*** Begin Patch\n*** Add File: a.txt\n+hello\n*** End Patch\n";
    let literal = serde_json::to_string(payload).expect("literal");
    let source = format!("// @exec: {{}}\nawait tools.apply_patch({literal});");
    let invocation_start = source.find("tools.apply_patch").expect("invocation");
    let argument_start = invocation_start + "tools.apply_patch(".len();
    let argument_end = argument_start + literal.len();
    let span = |start: usize, end: usize| SourceDescriptionSpan {
        source_id: 0,
        start_byte: start,
        end_byte: end,
        start_line: 2,
        end_line: 2,
        start_column: start - "// @exec: {}\n".len() + 1,
    };
    let path_start = payload.find("a.txt").expect("path");
    let line_start = payload.find("+hello").expect("line");
    let evidence = |start, end| json!({"patchId":0, "range":{"start":start,"end":end}});
    serde_json::from_value(json!({
        "disposition":"completeSyntax",
        "sources":[{"id":0,"language":"typeScript","parent":null,"text":source}],
        "references":[{"kind":"invocationCall","evidence":span(invocation_start,argument_end+1),"literal":null}],
        "patches":[{
            "id":0,"invocation":span(invocation_start,argument_end+1),
            "argument":span(argument_start,argument_end),"payload":payload,
            "files":[{"kind":"add", "path":{"spelling":"a.txt","evidence":evidence(path_start,path_start+5)},
                "lines":[{"prefix":"plus","evidence":evidence(line_start,line_start+6),"text":evidence(line_start+1,line_start+6)}]}]
        }]
    })).expect("source fixture")
}

fn plain(lines: Vec<Line<'static>>) -> String {
    lines
        .into_iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn compact_parent_and_submitted_detail_never_claim_application() {
    let cell =
        SourceDescriptionCell::new("source-1".to_owned(), submitted_patch()).expect("source cell");
    let compact = plain(cell.display_lines(100));
    assert!(compact.contains("Source · 1 references · 1 submitted patches"));
    assert!(!compact.contains("hello"));
    let detail = plain(cell.transcript_lines(100));
    assert!(detail.contains("Submitted patch 1 · source text"));
    assert!(detail.contains("*** Add File: a.txt"));
    assert!(detail.contains("+hello"));
    assert!(!detail.contains("Applied"));
    assert_eq!(cell.activity_ids(), vec!["source:source-1"]);
}

#[test]
fn malformed_patch_relationship_discards_all_detail() {
    let mut description = submitted_patch();
    let SourceDescription::CompleteSyntax { patches, .. } = &mut description else {
        unreachable!();
    };
    patches[0].argument.source_id = 7;
    assert!(SourceDescriptionCell::new("source-1".to_owned(), description).is_none());
}

#[test]
fn recognized_secret_discards_patch_and_source_detail() {
    let mut description = submitted_patch();
    let SourceDescription::CompleteSyntax { sources, .. } = &mut description else {
        unreachable!();
    };
    sources[0].text.push_str(" // sk-abcdefghijklmnopqrstuvwx");
    assert!(SourceDescriptionCell::new("source-1".to_owned(), description).is_none());
}

#[test]
fn restored_source_uses_the_live_renderer() {
    let description = submitted_patch();
    let live =
        SourceDescriptionCell::new("source-1".to_owned(), description.clone()).expect("live cell");
    let item = ThreadItem::SourceDescription {
        id: "source-1".to_owned(),
        call_id: "call-1".to_owned(),
        description,
    };
    let restored = crate::thread_transcript::thread_items_to_transcript_cells(
        None,
        &test_path_buf("/workspace").abs(),
        vec![item],
        crate::thread_transcript::RawReasoningVisibility::Hidden,
        None,
    );
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].display_lines(80), live.display_lines(80));
    assert_eq!(
        restored[0].transcript_hyperlink_lines(80),
        live.transcript_hyperlink_lines(80)
    );
    assert_eq!(restored[0].activity_ids(), live.activity_ids());
}

#[test]
fn fallback_and_empty_description_add_no_replay_cells() {
    let mut empty = submitted_patch();
    if let SourceDescription::CompleteSyntax {
        references,
        patches,
        ..
    } = &mut empty
    {
        references.clear();
        patches.clear();
    }
    let fallback = SourceDescription::Fallback {
        reason: codex_protocol::source_description::SourceDescriptionFallback::UnsupportedSource,
    };
    for description in [empty, fallback] {
        assert!(SourceDescriptionCell::new("source-1".to_owned(), description.clone()).is_none());
        let restored = crate::thread_transcript::thread_items_to_transcript_cells(
            None,
            &test_path_buf("/workspace").abs(),
            [ThreadItem::SourceDescription {
                id: "source-1".to_owned(),
                call_id: "call-1".to_owned(),
                description,
            }],
            crate::thread_transcript::RawReasoningVisibility::Hidden,
            None,
        );
        assert!(restored.is_empty());
    }
}

fn extension_description() -> SourceDescription {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../protocol/tests/fixtures/source_description_extensions.json"
    )))
    .expect("authored source-extension wire fixture")
}

#[test]
fn new_source_kinds_keep_the_same_plain_live_and_restored_presentation() {
    let description = extension_description();
    let live = SourceDescriptionCell::new("extensions".to_owned(), description.clone())
        .expect("validated source cell");
    let workspace = tempfile::tempdir().expect("temporary workspace");
    // A real eligible filename still does not turn a source spelling into a file link.
    std::fs::write(workspace.path().join("δ.txt"), "inert fixture").expect("fixture file");
    let restored = crate::thread_transcript::thread_items_to_transcript_cells(
        None,
        &workspace.path().to_path_buf().abs(),
        vec![ThreadItem::SourceDescription {
            id: "extensions".to_owned(),
            call_id: "source-call".to_owned(),
            description,
        }],
        crate::thread_transcript::RawReasoningVisibility::Hidden,
        None,
    );
    assert_eq!(restored.len(), 1);
    let expected = concat!(
        "Source · 13 references · 0 submitted patches\n",
        "Invocation spelling · source 0:1:7\n",
        "Invocation spelling · source 1:1:1\n",
        "cd\n",
        "Environment assignment spelling · source 1:1:1\n",
        "LANG=C\n",
        "Directory operand · source 1:1:14\n",
        "δ\n",
        "Shell connector spelling · source 1:1:19\n",
        "&&\n",
        "Invocation spelling · source 1:1:22\n",
        "cat\n",
        "Shell redirection operand · source 1:1:26\n",
        "δ.out\n",
        "Invocation spelling · source 1:2:1\n",
        "cat\n",
        "Literal heredoc data · source 1:3:1\n",
        "Invocation spelling · source 1:4:1\n",
        "python\n",
        "Open call spelling · source 2:1:1\n",
        "δ.txt\n",
        "Read call spelling · source 2:1:1\n",
        "δ.txt\n",
        "Transform spelling · source 2:1:1\n",
        "δ.txt",
    );
    let expected_transcript = expected
        .lines()
        .map(|line| format!("  {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    for cell in [&live as &dyn HistoryCell, restored[0].as_ref()] {
        // Exact text excludes applied/success badges, output claims and synthesized diffs.
        assert_eq!(plain(cell.raw_lines()), expected);
        assert_eq!(
            plain(cell.display_lines(160)),
            "• Source · 13 references · 0 submitted patches"
        );
        assert_eq!(plain(cell.transcript_lines(160)), expected_transcript);
        assert_eq!(cell.activity_ids(), vec!["source:extensions"]);
        assert_eq!(
            cell.activity_disclosure(160),
            Some(ActivityDisclosure::Generic)
        );
        for width in [24, 80, 160] {
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
fn source_extension_disclosure_failure_omits_the_entire_live_and_restored_cell() {
    let mut description = extension_description();
    let SourceDescription::CompleteSyntax { sources, .. } = &mut description else {
        panic!("complete source fixture");
    };
    sources[1].text.push_str("# sk-abcdefghijklmnopqrstuvwx\n");
    assert!(SourceDescriptionCell::new("extensions".to_owned(), description.clone()).is_none());
    let restored = crate::thread_transcript::thread_items_to_transcript_cells(
        None,
        &test_path_buf("/workspace").abs(),
        vec![ThreadItem::SourceDescription {
            id: "extensions".to_owned(),
            call_id: "source-call".to_owned(),
            description,
        }],
        crate::thread_transcript::RawReasoningVisibility::Hidden,
        None,
    );
    assert!(restored.is_empty());
}
