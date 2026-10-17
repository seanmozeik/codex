//! Authored wire states check disclosure, not program semantics or execution.

use super::description;
use super::plain;
use crate::history_cell::HistoryCell;
use crate::history_cell::SourceDescriptionCell;
use crate::test_support::PathBufExt;
use crate::test_support::test_path_buf;
use codex_app_server_protocol::ThreadItem;
use codex_protocol::source_description::SelectiveEditDetails;
use codex_protocol::source_description::SelectiveLaterContext;
use codex_protocol::source_description::SelectivePythonFinalState;
use codex_protocol::source_description::SourceDescription;
use pretty_assertions::assert_eq;

fn assert_compact_uncertainty(context: SelectiveLaterContext, expected: &str) {
    let mut input = description();
    let SourceDescription::SelectiveFindings { edits, .. } = &mut input else {
        panic!("authored selective fixture");
    };
    edits[0].later_context = context;
    let SelectiveEditDetails::Python { prediction, .. } = &mut edits[0].details else {
        panic!("authored Python fixture");
    };
    prediction.final_state = match context {
        SelectiveLaterContext::Unobserved => SelectivePythonFinalState::NotEstablished,
        SelectiveLaterContext::OpaqueSuffix => SelectivePythonFinalState::OpaqueSuccessor,
        SelectiveLaterContext::KnownLaterTouch => SelectivePythonFinalState::KnownLaterTouch,
        SelectiveLaterContext::PossibleAliasLaterTouch => {
            SelectivePythonFinalState::PossibleAliasLaterTouch
        }
    };
    let live = SourceDescriptionCell::new("uncertainty".to_owned(), input.clone())
        .expect("live conditional preview");
    let item = ThreadItem::SourceDescription {
        id: "uncertainty".to_owned(),
        call_id: "call".to_owned(),
        description: input,
    };
    let encoded = serde_json::to_vec(&item).expect("wire serialization");
    let decoded: ThreadItem = serde_json::from_slice(&encoded).expect("wire roundtrip");
    let restored = crate::thread_transcript::thread_items_to_transcript_cells(
        None,
        &test_path_buf("/workspace").abs(),
        vec![decoded],
        crate::thread_transcript::RawReasoningVisibility::Hidden,
        None,
    );
    assert_eq!(restored.len(), 1);
    for cell in [&live as &dyn HistoryCell, restored[0].as_ref()] {
        let compact = plain(cell.display_lines(160));
        for text in [
            "Conditional diff · before execution · operation point",
            expected,
            "-old",
            "+new",
        ] {
            assert!(compact.contains(text), "missing {text}: {compact}");
        }
        assert!(
            compact.find(expected).expect("uncertainty") < compact.find("-old").expect("diff body"),
            "qualify the preview before showing changed bytes"
        );
        for forbidden in ["Applied", "Succeeded", "Observed diff"] {
            assert!(!compact.contains(forbidden), "unexpected {forbidden}");
        }
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
fn opaque_suffix_warning_survives_compact_wire_restoration() {
    assert_compact_uncertainty(
        SelectiveLaterContext::OpaqueSuffix,
        "Final state unknown: later unsupported code may change it.",
    );
}

#[test]
fn same_path_later_touch_warns_beside_the_earlier_diff() {
    assert_compact_uncertainty(
        SelectiveLaterContext::KnownLaterTouch,
        "Final state unknown: a later operation targets this path.",
    );
}

#[test]
fn possible_alias_later_touch_does_not_claim_different_files_are_independent() {
    assert_compact_uncertainty(
        SelectiveLaterContext::PossibleAliasLaterTouch,
        "Final state unknown: a later target may alias this path.",
    );
}

#[test]
fn no_known_suffix_still_does_not_establish_final_file_state() {
    assert_compact_uncertainty(
        SelectiveLaterContext::Unobserved,
        "Final file state is unobserved.",
    );
}

#[test]
fn empty_diff_keeps_operation_scope_and_opaque_suffix_warning() {
    let mut input = description();
    let SourceDescription::SelectiveFindings { edits, .. } = &mut input else {
        panic!("authored selective fixture");
    };
    edits[0].later_context = SelectiveLaterContext::OpaqueSuffix;
    let snapshot = edits[0].snapshot.as_mut().expect("authored snapshot");
    snapshot.after = snapshot.before.clone().expect("existing target");
    snapshot.unified_diff.clear();
    let cell = SourceDescriptionCell::new("unchanged".to_owned(), input)
        .expect("operation with unchanged bytes");
    let compact = plain(cell.display_lines(160));
    for text in [
        "Conditional diff · before execution · operation point",
        "Final state unknown: later unsupported code may change it.",
        "No byte change at this operation point.",
    ] {
        assert!(compact.contains(text), "missing {text}: {compact}");
    }
}
