//! Parser fixtures exercise conversion only; submitted source is never executed.

use super::activity;
use super::description;
use super::wire;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::time::Duration;

pub(super) fn analyze(source: &str) -> activity::SelectiveEditAnalysis {
    let input = activity::SelectiveEditInput::from_source(
        "conditional".into(),
        activity::Language::Python,
        source,
        Some("/workspace"),
        activity::AnalysisLimits::new(32_768, Duration::from_secs(2)),
    )
    .expect("bounded authored input");
    activity::Analyzer::new()
        .expect("native parsers")
        .forecast_edits(input)
}

#[test]
fn conditional_conversion_keeps_open_and_write_distinct_from_final_state() {
    let result = analyze("open('note.txt', 'w').write('new')\nopaque()\n");
    let converted = description(&result);
    let wire::SourceDescription::SelectiveFindings {
        sources,
        edits,
        barriers,
    } = &converted
    else {
        panic!("conditional native result must stay conditional");
    };
    assert_eq!(sources.len(), 1);
    assert_eq!(edits.len(), 2);
    assert!(!barriers.is_empty());
    assert!(
        edits
            .iter()
            .all(|edit| edit.later_context == wire::SelectiveLaterContext::OpaqueSuffix)
    );
    let first = serde_json::to_value(&edits[0]).expect("first operation");
    let second = serde_json::to_value(&edits[1]).expect("second operation");
    assert_eq!(
        first["details"]["prediction"]["operation"]["type"],
        json!("truncate")
    );
    assert_eq!(
        second["details"]["prediction"]["operation"]["type"],
        json!("writeText")
    );
    assert_eq!(
        second["details"]["prediction"]["operation"]["content"]["value"],
        json!("new")
    );
    assert_eq!(
        second["details"]["prediction"]["target"],
        json!({"path":"note.txt","cwd":"/workspace"})
    );
    assert!(
        !second["details"]["prediction"]["predecessorConditions"]
            .as_array()
            .expect("conditions")
            .is_empty()
    );
    let encoded = serde_json::to_value(&converted).expect("wire serialization");
    assert_eq!(
        serde_json::from_value::<wire::SourceDescription>(encoded).expect("wire round trip"),
        converted
    );
}

#[test]
fn read_recipe_keeps_local_identity_epoch_and_runtime_assumptions() {
    let result = analyze(concat!(
        "from pathlib import Path\n",
        "p = Path('note.txt')\n",
        "s = p.read_text(encoding='utf-8')\n",
        "p.write_text(s.replace('old', 'new'), encoding='utf-8')\n",
    ));
    let converted = serde_json::to_value(description(&result)).expect("converted recipe");
    assert_eq!(converted["disposition"], json!("selectiveFindings"));
    let write = converted["edits"]
        .as_array()
        .expect("edits")
        .iter()
        .find(|edit| edit["details"]["prediction"]["operation"]["type"] == "writeText")
        .expect("write forecast");
    let content = &write["details"]["prediction"]["operation"]["content"];
    assert_eq!(content["type"], json!("replace"));
    assert_eq!(content["old"], json!("old"));
    assert_eq!(content["new"], json!("new"));
    assert_eq!(content["input"]["type"], json!("read"));
    assert_eq!(content["input"]["readId"], json!(0));
    assert_eq!(content["input"]["filesystemEpoch"], json!(0));
    assert_eq!(content["input"]["evidence"]["sourceId"], json!(0));
    assert!(
        write["details"]["assumptions"]
            .as_array()
            .expect("assumptions")
            .contains(&json!("noConcurrentMutation"))
    );
}

#[test]
fn shared_authored_fixture_passes_disclosure_but_sensitive_forecast_is_omitted() {
    let value: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../protocol/tests/fixtures/source_description_selective.json"
    )))
    .expect("shared authored fixture");
    let fixture: wire::SourceDescription = serde_json::from_value(value).expect("typed fixture");
    assert_eq!(
        super::source_description_for_display(fixture.clone()),
        fixture
    );
    let result = analyze("open('note.txt', 'w').write('sk-abcdefghijklmnopqrstuvwx')\n");
    assert_eq!(
        description(&result),
        wire::SourceDescription::Fallback {
            reason: wire::SourceDescriptionFallback::Presentation,
        }
    );
}

#[test]
fn known_later_touch_keeps_opaque_python_prediction_and_barrier() {
    let prefix = concat!(
        "with open('note.txt', 'r+', encoding='utf-8', newline='') as f:\n",
        "    text = f.read()\n",
        "    f.seek(0)\n",
        "    f.write('new')\n",
        "    f.flush()\n",
    );
    let baseline = description(&analyze(prefix));
    let source = format!("{prefix}    f.truncate(3)\n");
    let converted = description(&analyze(&source));
    let wire::SourceDescription::SelectiveFindings {
        edits: baseline_edits,
        barriers: baseline_barriers,
        ..
    } = baseline
    else {
        panic!("baseline logical forecast");
    };
    let wire::SourceDescription::SelectiveFindings {
        edits, barriers, ..
    } = &converted
    else {
        panic!("later touch must retain the conditional forecast");
    };

    assert_eq!(baseline_edits.len(), 1);
    assert_eq!(edits.len(), 1);
    assert_eq!(barriers.len(), 1);
    assert_eq!(barriers, &baseline_barriers);
    assert!(matches!(
        &barriers[0].reason,
        wire::SelectiveBarrierReason::Python {
            reason: wire::SelectivePythonBarrierReason::UnsupportedOperation,
        }
    ));
    assert_eq!(
        baseline_edits[0].later_context,
        wire::SelectiveLaterContext::OpaqueSuffix
    );
    let mut expected = baseline_edits[0].clone();
    expected.later_context = wire::SelectiveLaterContext::KnownLaterTouch;
    assert_eq!(&edits[0], &expected);
    assert!(edits[0].snapshot.is_none());
    let wire::SelectiveEditDetails::Python { prediction, .. } = &edits[0].details else {
        panic!("retained Python prediction");
    };
    assert_eq!(
        prediction.final_state,
        wire::SelectivePythonFinalState::OpaqueSuccessor
    );
    let wire::SelectivePythonOperation::LogicalWrite(write) = &prediction.operation else {
        panic!("later context must not turn a logical write into physical replay");
    };
    assert_eq!(write.base_filesystem_epoch, 0);
    assert_eq!(write.stream_revision, 0);
    assert_eq!(
        write.required_result,
        wire::SelectivePythonWriteResultCondition::FullCharacterCount
    );
    assert_eq!(
        serde_json::from_value::<wire::SourceDescription>(
            serde_json::to_value(&converted).expect("wire serialization"),
        )
        .expect("wire round trip"),
        converted
    );
}
