//! Actual handler-produced events; the recording runtime never evaluates source.

mod recording_runtime;

use codex_app_server_protocol::ThreadHistoryBuilder;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::build_turns_from_rollout_items;
use codex_app_server_protocol::source_description_for_display;
use codex_code_mode::CodeModeToolKind;
use codex_protocol::items::TurnItem;
use codex_protocol::protocol::Event;
use codex_protocol::protocol::EventMsg;
use codex_protocol::source_description::SelectiveLaterContext;
use codex_protocol::source_description::SelectiveSnapshotScope;
use codex_protocol::source_description::SourceDescription;
use codex_protocol::source_description::SourceDescriptionFallback;
use codex_rollout::RolloutItem;
use pretty_assertions::assert_eq;
use recording_runtime::Fixture;
use recording_runtime::INERT_HANDOFF;
use recording_runtime::Observation;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

const PYTHON: &str = "from pathlib import Path\np = Path('file')\ns = p.read_text(encoding='utf-8')\np.write_text(s.replace('old', 'new'), encoding='utf-8')";
const NODE: &str = "const fs = require('fs'); const s = fs.readFileSync('file', 'utf8'); fs.writeFileSync('file', s.replace('old', 'new'), 'utf8');";
const CANCELLED: &str = "Code execution was cancelled before starting.";
static PUBLICATION_LOCK: Mutex<()> = Mutex::const_new(());

fn invocation(script: &str, interpreter: &str) -> String {
    let command = serde_json::to_string(&format!("{interpreter} - <<'CODE'\n{script}\nCODE\n"))
        .expect("authored command");
    format!("await tools.exec_command({{cmd:{command}, login:false}});")
}

fn assert_handoff(observed: &Observation, expected_source: &str) {
    assert_eq!(observed.request.tool_call_id, "publication-call");
    assert_eq!(observed.request.source, expected_source);
    assert_eq!(observed.request.yield_time_ms, Some(17));
    assert_eq!(observed.request.max_output_tokens, Some(91));
    let bindings = observed
        .request
        .enabled_tools
        .iter()
        .map(|tool| (tool.name.as_str(), tool.kind))
        .collect::<Vec<_>>();
    assert_eq!(
        bindings,
        [
            ("apply_patch", CodeModeToolKind::Freeform),
            ("exec_command", CodeModeToolKind::Function),
        ]
    );
}

fn assert_publication_roundtrip(
    fixture: &Fixture,
    events: &[Event],
    persisted: &[RolloutItem],
) -> SourceDescription {
    let [turn_event, start_event, completion_event] = events else {
        panic!("turn start followed by exactly one source publication pair");
    };
    let EventMsg::TurnStarted(turn_started) = &turn_event.msg else {
        panic!("turn starts before source publication");
    };
    assert_eq!(turn_started.turn_id, fixture.step.turn.sub_id);
    let EventMsg::ItemStarted(started) = &start_event.msg else {
        panic!("source starts before source completion");
    };
    let EventMsg::ItemCompleted(completed) = &completion_event.msg else {
        panic!("source completion precedes runtime handoff");
    };
    assert_eq!(started.thread_id, fixture.session.thread_id);
    assert_eq!(completed.thread_id, fixture.session.thread_id);
    assert_eq!(started.turn_id, fixture.step.turn.sub_id);
    assert_eq!(completed.turn_id, fixture.step.turn.sub_id);
    let TurnItem::SourceDescription(start_item) = &started.item else {
        panic!("source start item");
    };
    let TurnItem::SourceDescription(item) = &completed.item else {
        panic!("source completion item");
    };
    assert_eq!(start_item, item);
    assert_eq!(item.call_id, "publication-call");
    assert_ne!(item.id, item.call_id);
    assert_eq!(
        source_description_for_display(item.description.clone()),
        item.description,
        "actual produced description passes display validation"
    );

    let live_item = ThreadItem::from(completed.item.clone());
    let encoded = serde_json::to_vec(&live_item).expect("actual item serialization");
    let decoded: ThreadItem = serde_json::from_slice(&encoded).expect("actual item restoration");
    assert_eq!(decoded, live_item);
    let mut live = ThreadHistoryBuilder::new();
    live.handle_event(&turn_event.msg);
    live.handle_event(&start_event.msg);
    let started_turn = live.active_turn_snapshot().expect("published turn start");
    assert_eq!(started_turn.id, fixture.step.turn.sub_id);
    assert!(started_turn.items.is_empty(), "start is not history");
    live.handle_event(&completion_event.msg);
    let live_turns = live.finish();
    assert_eq!(live_turns.len(), 1);
    assert_eq!(live_turns[0].id, fixture.step.turn.sub_id);
    assert_eq!(live_turns[0].items, [live_item.clone()]);

    // These are the real store's records, not reconstructed event fixtures.
    let encoded = serde_json::to_vec(persisted).expect("persisted record serialization");
    let decoded: Vec<RolloutItem> =
        serde_json::from_slice(&encoded).expect("persisted record restoration");
    let restored = build_turns_from_rollout_items(&decoded);
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].id, fixture.step.turn.sub_id);
    assert_eq!(restored[0].items, [live_item]);
    item.description.clone()
}

fn assert_preview(description: &SourceDescription, source: &str, later: SelectiveLaterContext) {
    let SourceDescription::SelectiveFindings { sources, edits, .. } = description else {
        panic!("actual handler-produced selective preview");
    };
    assert_eq!(sources[0].text, source, "retain raw submitted bytes");
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].later_context, later);
    let snapshot = edits[0]
        .snapshot
        .as_ref()
        .expect("automatic entry snapshot");
    assert_eq!(snapshot.scope, SelectiveSnapshotScope::OperationPoint);
    assert_eq!(snapshot.before.as_deref(), Some("old"));
    assert_eq!(snapshot.after, "new");
}

#[tokio::test]
async fn source_description_publication_precedes_inert_runtime_and_survives_store_reduction() {
    let _guard = PUBLICATION_LOCK.lock().await;
    for (script, interpreter) in [(PYTHON, "python3"), (NODE, "node")] {
        let fixture = Fixture::new().await;
        let body = invocation(script, interpreter);
        let source = format!("// @exec: {{\"yield_time_ms\":17,\"max_output_tokens\":91}}\n{body}");
        assert_eq!(
            fixture.invoke(&source, CancellationToken::new()).await,
            INERT_HANDOFF
        );
        let observed = fixture.take_observation().expect("actual runtime request");
        assert_handoff(&observed, &body);
        let description =
            assert_publication_roundtrip(&fixture, &observed.events, &observed.persisted);
        assert_preview(&description, &source, SelectiveLaterContext::Unobserved);
        let forwarded = observed.preempt.expect("forwarded step token");
        assert!(
            !forwarded.is_cancelled(),
            "forwarded token starts uncancelled"
        );
        fixture
            .step
            .preempt
            .as_ref()
            .expect("step preemption")
            .cancel();
        assert!(
            forwarded.is_cancelled(),
            "original cancellation reaches the runtime token"
        );
        assert_eq!(fixture.bytes().await, b"old");
    }
}

#[tokio::test]
async fn opaque_predecessor_fallback_keeps_original_runtime_request_and_persisted_identity() {
    let _guard = PUBLICATION_LOCK.lock().await;
    for (script, interpreter) in [(PYTHON, "python3"), (NODE, "node")] {
        let fixture = Fixture::new().await;
        let body = format!("opaque();\n{}", invocation(script, interpreter));
        let source = format!("// @exec: {{\"yield_time_ms\":17,\"max_output_tokens\":91}}\n{body}");
        assert_eq!(
            fixture.invoke(&source, CancellationToken::new()).await,
            INERT_HANDOFF
        );
        let observed = fixture
            .take_observation()
            .expect("fallback still hands off original code");
        assert_handoff(&observed, &body);
        assert_eq!(
            assert_publication_roundtrip(&fixture, &observed.events, &observed.persisted),
            SourceDescription::Fallback {
                reason: SourceDescriptionFallback::UnsupportedSource
            }
        );
        assert_eq!(fixture.bytes().await, b"old");
    }
}

#[tokio::test]
async fn opaque_suffix_operation_point_survives_real_publication_and_persisted_wire() {
    let _guard = PUBLICATION_LOCK.lock().await;
    for (script, interpreter) in [(PYTHON, "python3"), (NODE, "node")] {
        let fixture = Fixture::new().await;
        let body = format!("{}\nopaque();", invocation(script, interpreter));
        let source = format!("// @exec: {{\"yield_time_ms\":17,\"max_output_tokens\":91}}\n{body}");
        assert_eq!(
            fixture.invoke(&source, CancellationToken::new()).await,
            INERT_HANDOFF
        );
        let observed = fixture.take_observation().expect("actual runtime request");
        assert_handoff(&observed, &body);
        let description =
            assert_publication_roundtrip(&fixture, &observed.events, &observed.persisted);
        assert_preview(&description, &source, SelectiveLaterContext::OpaqueSuffix);
        assert_eq!(fixture.bytes().await, b"old");
    }
}

#[tokio::test]
async fn cancelled_handler_publishes_nothing_and_never_enters_runtime() {
    let _guard = PUBLICATION_LOCK.lock().await;
    let fixture = Fixture::new().await;
    let setup_events = fixture.remaining_events();
    assert_eq!(setup_events.len(), 1);
    assert!(matches!(
        &setup_events[0].msg,
        EventMsg::TurnStarted(started) if started.turn_id == fixture.step.turn.sub_id
    ));
    let setup_records = fixture.persisted().await;
    let setup_turns = build_turns_from_rollout_items(&setup_records);
    assert_eq!(setup_turns.len(), 1);
    assert_eq!(setup_turns[0].id, fixture.step.turn.sub_id);
    assert!(setup_turns[0].items.is_empty());
    let setup_wire = serde_json::to_value(&setup_records).expect("setup history serialization");
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    assert_eq!(
        fixture
            .invoke(&invocation(PYTHON, "python3"), cancellation)
            .await,
        CANCELLED
    );
    assert!(fixture.take_observation().is_none());
    assert!(fixture.remaining_events().is_empty());
    assert_eq!(
        serde_json::to_value(fixture.persisted().await).expect("post-cancellation history"),
        setup_wire,
        "cancelled handler appends no records to the started turn"
    );
    assert_eq!(fixture.bytes().await, b"old");
}

#[tokio::test]
async fn cancellation_during_source_completion_preserves_publication_without_runtime_entry() {
    let _guard = PUBLICATION_LOCK.lock().await;
    let mut fixture = Fixture::new().await;
    let cancellation = CancellationToken::new();
    fixture.cancel_during_source_completion(cancellation.clone());
    let source = invocation(PYTHON, "python3");
    assert_eq!(
        fixture.invoke(&source, cancellation.clone()).await,
        CANCELLED
    );
    assert!(cancellation.is_cancelled());
    assert!(fixture.take_observation().is_none());
    let description = assert_publication_roundtrip(
        &fixture,
        &fixture.remaining_events(),
        &fixture.persisted().await,
    );
    assert_preview(&description, &source, SelectiveLaterContext::Unobserved);
    assert_eq!(fixture.bytes().await, b"old");
}
