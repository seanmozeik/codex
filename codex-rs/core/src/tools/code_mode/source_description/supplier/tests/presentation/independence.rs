//! Native presentation tests; embedded Python, Node and shell source stays inert.

use super::super::Fixture;
use super::super::REPLACE;
use crate::tools::code_mode::source_description::describe;
use crate::tools::code_mode::source_description::supplier::SnapshotSupplier;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::source_description_for_display;
use codex_code_mode::CodeModeToolKind;
use codex_code_mode::ToolDefinition;
use codex_protocol::ToolName;
use codex_protocol::source_description::SelectiveEditDetails;
use codex_protocol::source_description::SelectiveLaterContext;
use codex_protocol::source_description::SelectivePythonFinalState;
use codex_protocol::source_description::SelectiveSnapshotScope;
use codex_protocol::source_description::SourceDescription;
use codex_protocol::source_description::SourceDescriptionFallback;
use pretty_assertions::assert_eq;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tokio::sync::Mutex;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

const NODE_REPLACE: &str = "const fs = require('fs'); const s = fs.readFileSync('file', 'utf8'); fs.writeFileSync('file', s.replace('old', 'new'), 'utf8');";
static PRESENTATION_LOCK: Mutex<()> = Mutex::const_new(());

fn invocation(script: &str, interpreter: &str) -> String {
    let command = serde_json::to_string(&format!("{interpreter} - <<'CODE'\n{script}\nCODE\n"))
        .expect("authored command");
    format!("await tools.exec_command({{cmd:{command}, login:false}});")
}

async fn present(fixture: &Fixture, source: &str) -> (SourceDescription, usize) {
    // These cases share the native worker; queue before starting its deadline.
    let _guard = PRESENTATION_LOCK.lock().await;
    let tools = [
        ("apply_patch", CodeModeToolKind::Freeform),
        ("exec_command", CodeModeToolKind::Function),
    ]
    .into_iter()
    .map(|(name, kind)| ToolDefinition {
        name: name.to_owned(),
        tool_name: ToolName::plain(name),
        description: String::new(),
        kind,
        input_schema: None,
        input_schema_max_bytes: None,
        output_schema: None,
    })
    .collect::<Vec<_>>();
    let supplier = SnapshotSupplier::new(fixture.environment.clone());
    let attempts = Arc::clone(&supplier.capture_attempts);
    let description = describe(
        "independence-fixture",
        source,
        &tools,
        &CancellationToken::new(),
        Arc::new(Semaphore::new(1)),
        Some(supplier),
    )
    .await
    .expect("native presentation result");
    if let SourceDescription::SelectiveFindings { sources, .. } = &description {
        assert_eq!(
            sources[0].text, source,
            "preserve the submitted source bytes"
        );
    }
    let item = ThreadItem::SourceDescription {
        id: "independence-item".to_owned(),
        call_id: "independence-fixture".to_owned(),
        description: description.clone(),
    };
    let encoded = serde_json::to_vec(&item).expect("wire item serialization");
    let decoded: ThreadItem = serde_json::from_slice(&encoded).expect("wire item roundtrip");
    let ThreadItem::SourceDescription {
        description: restored,
        ..
    } = decoded
    else {
        panic!("source description item");
    };
    assert_eq!(source_description_for_display(restored), description);
    (description, attempts.load(Ordering::Relaxed))
}

#[tokio::test]
async fn opaque_outer_predecessor_prevents_every_capture_attempt() {
    let fixture = Fixture::new().await;
    fixture.write("file", b"old").await;
    for (script, interpreter) in [(REPLACE, "python3"), (NODE_REPLACE, "node")] {
        let source = format!("opaque();\n{}", invocation(script, interpreter));
        let (description, attempts) = present(&fixture, &source).await;
        assert_eq!(
            description,
            SourceDescription::Fallback {
                reason: SourceDescriptionFallback::UnsupportedSource
            }
        );
        assert_eq!(
            attempts, 0,
            "no target or read capture after opaque execution"
        );
    }
    assert_eq!(fixture.bytes("file").await, b"old");
}

#[tokio::test]
async fn opaque_outer_suffix_keeps_only_the_earlier_operation_point() {
    let fixture = Fixture::new().await;
    fixture.write("file", b"old").await;
    for (script, interpreter) in [(REPLACE, "python3"), (NODE_REPLACE, "node")] {
        let child = invocation(script, interpreter);
        let source = format!("{child}\nopaque();\n{child}");
        let (description, attempts) = present(&fixture, &source).await;
        let SourceDescription::SelectiveFindings {
            edits, barriers, ..
        } = description
        else {
            panic!("retain an understood operation before the opaque suffix");
        };
        assert_eq!(edits.len(), 1, "never recover the later child");
        assert_eq!(barriers.len(), 1);
        assert_eq!(edits[0].later_context, SelectiveLaterContext::OpaqueSuffix);
        let preview = edits[0].snapshot.as_ref().expect("earlier operation point");
        assert_eq!(preview.scope, SelectiveSnapshotScope::OperationPoint);
        assert_eq!(preview.before.as_deref(), Some("old"));
        assert_eq!(preview.after, "new");
        assert!(attempts > 0);
    }
    assert_eq!(fixture.bytes("file").await, b"old");
}

#[tokio::test]
async fn opaque_python_suffix_preserves_inner_and_outer_uncertainty() {
    let fixture = Fixture::new().await;
    fixture.write("file", b"old").await;
    let script = format!("{REPLACE}\nopaque()\np.write_text('later', encoding='utf-8')");
    let (description, _) = present(&fixture, &invocation(&script, "python3")).await;
    let SourceDescription::SelectiveFindings {
        edits, barriers, ..
    } = description
    else {
        panic!("retain the preceding Python operation");
    };
    assert_eq!(edits.len(), 1);
    assert_eq!(barriers.len(), 1);
    assert_eq!(edits[0].later_context, SelectiveLaterContext::OpaqueSuffix);
    let SelectiveEditDetails::Python { prediction, .. } = &edits[0].details else {
        panic!("Python prediction");
    };
    assert_eq!(
        prediction.final_state,
        SelectivePythonFinalState::OpaqueSuccessor
    );
    assert_eq!(edits[0].snapshot.as_ref().expect("preview").after, "new");
    assert_eq!(fixture.bytes("file").await, b"old");
}

#[tokio::test]
async fn finite_pure_predecessor_keeps_its_completion_premise_and_later_diff() {
    let fixture = Fixture::new().await;
    fixture.write("file", b"old").await;
    let script = "from pathlib import Path\np = Path('file')\ns = p.read_text(encoding='utf-8')\ns.count('old')\np.write_text(s.replace('old', 'new'), encoding='utf-8')";
    let (description, _) = present(&fixture, &invocation(script, "python3")).await;
    let SourceDescription::SelectiveFindings {
        sources,
        edits,
        barriers,
    } = description
    else {
        panic!("pure predecessor leaves the later write understood");
    };
    assert_eq!(edits.len(), 1);
    assert!(barriers.is_empty());
    let SelectiveEditDetails::Python { prediction, .. } = &edits[0].details else {
        panic!("Python prediction");
    };
    assert!(prediction.predecessor_conditions.iter().any(|condition| {
        sources[condition.source_id]
            .text
            .get(condition.start_byte..condition.end_byte)
            == Some("s.count('old')")
    }));
    assert_eq!(edits[0].later_context, SelectiveLaterContext::Unobserved);
    assert_eq!(edits[0].snapshot.as_ref().expect("preview").after, "new");
    assert_eq!(fixture.bytes("file").await, b"old");
}

#[tokio::test]
async fn same_path_later_write_replays_without_claiming_the_first_diff_is_final() {
    let fixture = Fixture::new().await;
    fixture.write("file", b"old").await;
    let script = format!("{REPLACE}\np.write_text('last', encoding='utf-8')");
    let (description, _) = present(&fixture, &invocation(&script, "python3")).await;
    let SourceDescription::SelectiveFindings { edits, .. } = description else {
        panic!("two understood writes");
    };
    assert_eq!(edits.len(), 2);
    assert_eq!(
        edits[0].later_context,
        SelectiveLaterContext::KnownLaterTouch
    );
    assert_eq!(
        edits[0].snapshot.as_ref().expect("first preview").after,
        "new"
    );
    let later = edits[1].snapshot.as_ref().expect("replayed preview");
    assert_eq!(later.before.as_deref(), Some("new"));
    assert_eq!(later.after, "last");
    assert_eq!(later.replay_ordinals, [0]);
    assert_eq!(fixture.bytes("file").await, b"old");
}

#[tokio::test]
async fn different_path_later_write_retains_alias_uncertainty_without_fresh_snapshot() {
    let fixture = Fixture::new().await;
    fixture.write("file", b"old").await;
    let script = format!("{REPLACE}\nPath('other').write_text('last', encoding='utf-8')");
    let (description, _) = present(&fixture, &invocation(&script, "python3")).await;
    let SourceDescription::SelectiveFindings { edits, .. } = description else {
        panic!("two understood symbolic writes");
    };
    assert_eq!(edits.len(), 2);
    assert_eq!(
        edits[0].later_context,
        SelectiveLaterContext::PossibleAliasLaterTouch
    );
    assert_eq!(
        edits[0].snapshot.as_ref().expect("first preview").after,
        "new"
    );
    assert!(edits[1].snapshot.is_none());
    assert_eq!(fixture.bytes("file").await, b"old");
}

#[tokio::test]
async fn earlier_child_completion_does_not_authorize_a_later_capture() {
    let fixture = Fixture::new().await;
    fixture.write("file", b"old").await;
    let child = invocation(REPLACE, "python3");
    let (_, first_attempts) = present(&fixture, &child).await;
    let (description, attempts) = present(&fixture, &format!("{child}\n{child}")).await;
    let SourceDescription::SelectiveFindings { edits, .. } = description else {
        panic!("two conditional child forecasts");
    };
    assert_eq!(edits.len(), 2);
    assert!(edits[0].snapshot.is_some());
    assert!(!edits[1].predecessor_conditions.is_empty());
    assert!(edits[1].snapshot.is_none());
    assert!(first_attempts > 0);
    assert_eq!(
        attempts, first_attempts,
        "the second child must not begin capture"
    );
    assert_eq!(fixture.bytes("file").await, b"old");
}
