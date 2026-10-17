//! Source descriptions produced by real code-mode dispatch stay separate from patch execution.

use std::error::Error;
use std::sync::Arc;
use std::time::Duration;

use codex_core::TurnInputRequest;
use codex_core::config::Config;
use codex_core::config::Constrained;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::ToolCallSource;
use codex_extension_api::ToolLifecycleContributor;
use codex_extension_api::ToolLifecycleFuture;
use codex_extension_api::ToolStartInput;
use codex_features::Feature;
use codex_protocol::items::TurnItem;
use codex_protocol::models::PermissionProfile;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::PatchApplyStatus;
use codex_protocol::user_input::UserInput;
use codex_tools::ToolPayload;
use core_test_support::responses;
use core_test_support::responses::ResponseMock;
use core_test_support::skip_if_no_network;
use core_test_support::streaming_sse::StreamingSseChunk;
use core_test_support::streaming_sse::start_streaming_sse_server;
use core_test_support::test_codex::TestCodex;
use core_test_support::test_codex::TestCodexHarness;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use test_case::test_case;
use tokio::sync::Notify;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::time::timeout;

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

const EVENT_TIMEOUT: Duration = Duration::from_secs(60);
const CALL_ID: &str = "source-description-call";
const FILE_NAME: &str = "source-description-fixture.txt";
const PATCH: &str = "*** Begin Patch\n*** Update File: source-description-fixture.txt\n@@\n-before\n+after\n*** End Patch\n";

fn configure_source_fixture(config: &mut Config) {
    for feature in [
        Feature::CodeMode,
        Feature::CodeModeHost,
        Feature::CodeModeInterrupt,
    ] {
        config
            .features
            .enable(feature)
            .expect("enable code-mode fixture");
    }
    config.code_mode.disable_in_process_fallback = true;
    config.permissions.approval_policy = Constrained::allow_any(AskForApproval::Never);
    config
        .permissions
        .set_permission_profile(PermissionProfile::Disabled)
        .expect("set permissions for the disposable authored workspace");
}

#[derive(Debug)]
struct ObservedCall {
    call_id: String,
    payload: ToolPayload,
    source: ToolCallSource,
}

struct PatchDispatchGate {
    observed: mpsc::UnboundedSender<ObservedCall>,
    release: Arc<Notify>,
}

impl ToolLifecycleContributor for PatchDispatchGate {
    fn on_tool_start<'a>(&'a self, input: ToolStartInput<'a>) -> ToolLifecycleFuture<'a> {
        Box::pin(async move {
            let nested_patch = input.tool_name.name == "apply_patch"
                && matches!(&input.source, ToolCallSource::CodeMode { .. });
            if !nested_patch && input.call_id != CALL_ID {
                return;
            }
            self.observed
                .send(ObservedCall {
                    call_id: input.call_id.to_owned(),
                    payload: input.payload.clone(),
                    source: input.source,
                })
                .expect("dispatch observer should remain connected");
            if nested_patch {
                self.release.notified().await;
            }
        })
    }
}

struct GatedPatchTurn {
    harness: TestCodexHarness,
    observed: mpsc::UnboundedReceiver<ObservedCall>,
    release: Arc<Notify>,
    follow_up: ResponseMock,
}

impl GatedPatchTurn {
    async fn start(code: &str, initial_contents: &str) -> TestResult<Self> {
        let (observed_tx, observed) = mpsc::unbounded_channel();
        let release = Arc::new(Notify::new());
        let mut extensions = ExtensionRegistryBuilder::<Config>::new();
        extensions.tool_lifecycle_contributor(Arc::new(PatchDispatchGate {
            observed: observed_tx,
            release: Arc::clone(&release),
        }));
        let builder = test_codex()
            .with_model("test-gpt-5.1-codex")
            .with_code_mode_host_program(codex_utils_cargo_bin::cargo_bin("codex-code-mode-host")?)
            .with_extensions(Arc::new(extensions.build()))
            .with_config(configure_source_fixture);
        let harness = TestCodexHarness::with_builder(builder).await?;
        harness.write_file(FILE_NAME, initial_contents).await?;
        responses::mount_sse_once(
            harness.server(),
            responses::sse(vec![
                responses::ev_response_created("source-response"),
                responses::ev_custom_tool_call(CALL_ID, "exec", code),
                responses::ev_completed("source-response"),
            ]),
        )
        .await;
        let follow_up = responses::mount_sse_once(
            harness.server(),
            responses::sse(vec![
                responses::ev_assistant_message("source-done", "done"),
                responses::ev_completed("source-follow-up"),
            ]),
        )
        .await;
        harness
            .test()
            .codex
            .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                text: "Run the authored patch fixture.".to_owned(),
                text_elements: Vec::new(),
            }]))
            .await?;
        Ok(Self {
            harness,
            observed,
            release,
            follow_up,
        })
    }

    async fn inspect_before_patch(&mut self, code: &str, patch: &str) -> TestResult<Vec<EventMsg>> {
        let outer = timeout(EVENT_TIMEOUT, self.observed.recv())
            .await?
            .expect("outer handler should start");
        assert_eq!(outer.call_id, CALL_ID);
        assert_eq!(outer.source, ToolCallSource::Direct);
        assert_eq!(
            outer.payload,
            ToolPayload::Custom {
                input: code.to_owned()
            }
        );
        let nested = timeout(EVENT_TIMEOUT, self.observed.recv())
            .await?
            .expect("real code-mode host should dispatch the patch");
        assert_ne!(nested.call_id, CALL_ID);
        assert!(matches!(nested.source, ToolCallSource::CodeMode { .. }));
        assert_eq!(
            nested.payload,
            ToolPayload::Custom {
                input: patch.to_owned()
            }
        );

        let events = collect_until(self.harness.test(), |event| {
            matches!(event, EventMsg::ItemCompleted(completed)
                if matches!(&completed.item, TurnItem::SourceDescription(_)))
        })
        .await?;
        assert!(events.iter().all(|event| !is_patch_execution_event(event)));
        assert!(self.follow_up.requests().is_empty());
        Ok(events)
    }

    async fn complete(&self, events: &mut Vec<EventMsg>) -> TestResult {
        self.release.notify_one();
        events.extend(
            collect_until(self.harness.test(), |event| {
                matches!(event, EventMsg::TurnComplete(_))
            })
            .await?,
        );
        Ok(())
    }
}

async fn collect_until(
    test: &TestCodex,
    finished: impl Fn(&EventMsg) -> bool,
) -> TestResult<Vec<EventMsg>> {
    timeout(EVENT_TIMEOUT, async {
        let mut events = Vec::new();
        loop {
            let event = test.codex.next_event().await?.msg;
            let done = finished(&event);
            events.push(event);
            if done {
                return Ok(events);
            }
        }
    })
    .await?
}

fn is_patch_execution_event(event: &EventMsg) -> bool {
    matches!(
        event,
        EventMsg::PatchApplyBegin(_) | EventMsg::PatchApplyEnd(_) | EventMsg::TurnDiff(_)
    ) || matches!(event, EventMsg::ItemStarted(started) if matches!(&started.item, TurnItem::FileChange(_)))
        || matches!(event, EventMsg::ItemCompleted(completed) if matches!(&completed.item, TurnItem::FileChange(_)))
}

fn atomic_description(events: &[EventMsg]) -> Value {
    let started = events
        .iter()
        .filter_map(|event| match event {
            EventMsg::ItemStarted(started) => match &started.item {
                TurnItem::SourceDescription(item) => Some(item),
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();
    let completed = events
        .iter()
        .filter_map(|event| match event {
            EventMsg::ItemCompleted(completed) => match &completed.item {
                TurnItem::SourceDescription(item) => Some(item),
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        started.len(),
        1,
        "one atomic analysis start per submitted call"
    );
    assert_eq!(
        completed.len(),
        1,
        "one atomic analysis completion per submitted call"
    );
    assert_eq!(
        serde_json::to_value(started[0]).expect("serialize started source"),
        serde_json::to_value(completed[0]).expect("serialize completed source")
    );
    assert_eq!(completed[0].call_id, CALL_ID);
    assert_ne!(completed[0].id, CALL_ID);
    serde_json::to_value(&completed[0].description).expect("serialize actual source description")
}

fn assert_native_patch_succeeded(events: &[EventMsg]) {
    let ends = events
        .iter()
        .filter_map(|event| match event {
            EventMsg::PatchApplyEnd(end) => Some(end),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(ends.len(), 1);
    assert!(ends[0].success);
    assert_eq!(ends[0].status, PatchApplyStatus::Completed);
    assert_eq!(ends[0].changes.len(), 1);
    let file_items = events
        .iter()
        .filter_map(|event| match event {
            EventMsg::ItemCompleted(completed) => match &completed.item {
                TurnItem::FileChange(item) => Some(item),
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        file_items.len(),
        1,
        "source analysis must not synthesize a second applied-file item"
    );
    assert_eq!(file_items[0].id, ends[0].call_id);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, EventMsg::TurnDiff(diff)
        if diff.unified_diff.contains("-before") && diff.unified_diff.contains("+after")))
    );
}

#[derive(Clone, Copy)]
enum Envelope {
    Literal,
    Pragma,
    ExecSibling,
    UnsupportedSibling,
    ComputedPatch,
    InvalidPatch,
}

#[test_case(Envelope::Literal; "literal")]
#[test_case(Envelope::Pragma; "pragma")]
#[test_case(Envelope::ExecSibling; "supported_exec_sibling")]
#[test_case(Envelope::UnsupportedSibling; "unsupported_late_sibling")]
#[test_case(Envelope::ComputedPatch; "computed_patch_argument")]
#[test_case(Envelope::InvalidPatch; "invalid_patch")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn source_description_is_atomic_before_nested_patch_dispatch(
    envelope: Envelope,
) -> TestResult {
    skip_if_no_network!(Ok(()));
    let patch = if matches!(envelope, Envelope::InvalidPatch) {
        "*** Begin Patch\n*** Update File: source-description-fixture.txt\n@@\n-before\n+after\n"
    } else {
        PATCH
    };
    let literal = serde_json::to_string(patch)?;
    let invocation = format!("text(await tools.apply_patch({literal}));");
    let code = match envelope {
        Envelope::Pragma => format!("// @exec: {{\"max_output_tokens\": 256}}\n{invocation}"),
        Envelope::ExecSibling => {
            format!(
                "{invocation}\ntext(await tools.exec_command({{cmd:'cat {FILE_NAME}',login:false}}));"
            )
        }
        Envelope::UnsupportedSibling => format!("{invocation}\nMath.max(1, 2);"),
        Envelope::ComputedPatch => {
            format!("const patch = {literal}; text(await tools.apply_patch(patch));")
        }
        Envelope::Literal | Envelope::InvalidPatch => invocation,
    };
    let mut turn = GatedPatchTurn::start(&code, "before\n").await?;
    let mut events = turn.inspect_before_patch(&code, patch).await?;
    assert_eq!(turn.harness.read_file_text(FILE_NAME).await?, "before\n");
    let description = atomic_description(&events);
    if matches!(
        envelope,
        Envelope::Literal | Envelope::Pragma | Envelope::ExecSibling
    ) {
        assert_eq!(description["disposition"], "completeSyntax");
        assert_eq!(description["sources"][0]["text"], code);
        assert_eq!(
            description["patches"].as_array().expect("patch rows").len(),
            1
        );
        assert_eq!(description["patches"][0]["payload"], patch);
        let evidence = &description["patches"][0]["invocation"];
        let start = usize::try_from(evidence["startByte"].as_u64().expect("invocation start"))?;
        let end = usize::try_from(evidence["endByte"].as_u64().expect("invocation end"))?;
        assert_eq!(&code[start..end], format!("tools.apply_patch({literal})"));
        let source_count = if matches!(envelope, Envelope::ExecSibling) {
            2
        } else {
            1
        };
        assert_eq!(
            description["sources"]
                .as_array()
                .expect("source rows")
                .len(),
            source_count
        );
        if matches!(envelope, Envelope::ExecSibling) {
            let shell = &description["sources"][1];
            assert_eq!(shell["language"], "shell");
            assert_eq!(shell["text"], format!("cat {FILE_NAME}"));
            assert_eq!(shell["parent"]["sourceId"], 0);
            let parent_start = usize::try_from(
                shell["parent"]["startByte"]
                    .as_u64()
                    .expect("shell parent start"),
            )?;
            let parent_end = usize::try_from(
                shell["parent"]["endByte"]
                    .as_u64()
                    .expect("shell parent end"),
            )?;
            assert_eq!(
                &code[parent_start..parent_end],
                format!("tools.exec_command({{cmd:'cat {FILE_NAME}',login:false}})")
            );
            assert!(
                description["references"]
                    .as_array()
                    .expect("source references")
                    .iter()
                    .any(|reference| {
                        reference["evidence"]["sourceId"] == 1
                            && reference["kind"] == "invocationCall"
                            && reference["literal"] == "cat"
                    })
            );
        }
    } else {
        assert_eq!(
            description,
            json!({"disposition": "fallback", "reason": "unsupportedSource"})
        );
    }
    turn.complete(&mut events).await?;
    assert_eq!(
        atomic_description(&events),
        description,
        "runtime completion cannot replace the submitted description"
    );
    if matches!(envelope, Envelope::InvalidPatch) {
        assert_eq!(turn.harness.read_file_text(FILE_NAME).await?, "before\n");
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, EventMsg::TurnDiff(_)))
        );
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, EventMsg::PatchApplyEnd(end) if end.success))
        );
    } else {
        assert_eq!(turn.harness.read_file_text(FILE_NAME).await?, "after\n");
        assert_native_patch_succeeded(&events);
    }
    assert_eq!(turn.follow_up.requests().len(), 1);
    turn.harness.test().codex.shutdown_and_wait().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn identical_source_descriptions_do_not_predict_matching_file_context() -> TestResult {
    skip_if_no_network!(Ok(()));
    let code = format!(
        "text(await tools.apply_patch({}));",
        serde_json::to_string(PATCH)?
    );
    let mut descriptions = Vec::new();
    for initial in ["before\n", "different file contents\n"] {
        let mut turn = GatedPatchTurn::start(&code, initial).await?;
        let mut events = turn.inspect_before_patch(&code, PATCH).await?;
        assert_eq!(turn.harness.read_file_text(FILE_NAME).await?, initial);
        descriptions.push(atomic_description(&events));
        turn.complete(&mut events).await?;
        if initial == "before\n" {
            assert_native_patch_succeeded(&events);
            assert_eq!(turn.harness.read_file_text(FILE_NAME).await?, "after\n");
        } else {
            assert_eq!(turn.harness.read_file_text(FILE_NAME).await?, initial);
            assert!(
                !events
                    .iter()
                    .any(|event| matches!(event, EventMsg::TurnDiff(_)))
            );
            assert!(
                !events
                    .iter()
                    .any(|event| matches!(event, EventMsg::PatchApplyEnd(end) if end.success))
            );
            let output = turn
                .follow_up
                .single_request()
                .custom_tool_call_output(CALL_ID);
            assert!(
                output.to_string().contains("Failed to find expected lines"),
                "the real patch validator should reject mismatching context: {output}"
            );
        }
        turn.harness.test().codex.shutdown_and_wait().await?;
    }
    assert_eq!(descriptions[0]["disposition"], "completeSyntax");
    assert_eq!(descriptions[0], descriptions[1]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_after_source_completion_does_not_apply_the_gated_patch() -> TestResult {
    skip_if_no_network!(Ok(()));
    let code = format!(
        "text(await tools.apply_patch({}));",
        serde_json::to_string(PATCH)?
    );
    let mut turn = GatedPatchTurn::start(&code, "before\n").await?;
    let mut events = turn.inspect_before_patch(&code, PATCH).await?;
    let description = atomic_description(&events);
    turn.harness.test().codex.submit(Op::Interrupt).await?;
    events.extend(
        collect_until(turn.harness.test(), |event| {
            matches!(event, EventMsg::TurnAborted(_))
        })
        .await?,
    );
    turn.release.notify_one();
    turn.harness
        .test()
        .codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "Confirm the interrupted turn is finished.".to_owned(),
            text_elements: Vec::new(),
        }]))
        .await?;
    events.extend(
        collect_until(turn.harness.test(), |event| {
            matches!(event, EventMsg::TurnComplete(_))
        })
        .await?,
    );
    assert_eq!(turn.harness.read_file_text(FILE_NAME).await?, "before\n");
    assert_eq!(atomic_description(&events), description);
    assert!(!events.iter().any(is_patch_execution_event));
    turn.harness.test().codex.shutdown_and_wait().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn streamed_code_mode_arguments_do_not_publish_partial_source_items() -> TestResult {
    skip_if_no_network!(Ok(()));
    let code = format!(
        "text(await tools.apply_patch({}));",
        serde_json::to_string(PATCH)?
    );
    let (release_final, final_gate) = oneshot::channel();
    let split = code.len() / 2;
    let (server, _) = start_streaming_sse_server(vec![
        vec![
            StreamingSseChunk {
                gate: None,
                body: responses::sse(vec![
                    responses::ev_response_created("stream-source-response"),
                    json!({
                        "type": "response.output_item.added",
                        "item": {"type": "custom_tool_call", "call_id": CALL_ID, "name": "exec", "input": ""},
                    }),
                    json!({"type": "response.custom_tool_call_input.delta", "call_id": CALL_ID, "delta": &code[..split]}),
                    responses::ev_assistant_message("stream-checkpoint", "Waiting for the rest of the authored call."),
                ]),
            },
            StreamingSseChunk {
                gate: Some(final_gate),
                body: responses::sse(vec![
                    json!({"type": "response.custom_tool_call_input.delta", "call_id": CALL_ID, "delta": &code[split..]}),
                    responses::ev_custom_tool_call(CALL_ID, "exec", &code),
                    responses::ev_completed("stream-source-response"),
                ]),
            },
        ],
        vec![StreamingSseChunk {
            gate: None,
            body: responses::sse(vec![
                responses::ev_assistant_message("stream-source-done", "done"),
                responses::ev_completed("stream-source-follow-up"),
            ]),
        }],
    ]).await;
    let test = test_codex()
        .with_model("test-gpt-5.1-codex")
        .with_code_mode_host_program(codex_utils_cargo_bin::cargo_bin("codex-code-mode-host")?)
        .with_config(configure_source_fixture)
        .with_config(|config| {
            config
                .features
                .enable(Feature::ApplyPatchStreamingEvents)
                .expect("enable streaming fixture");
        })
        .build_with_streaming_server(&server)
        .await?;
    let file_path = test.workspace_path_uri(FILE_NAME)?;
    test.fs()
        .write_file(&file_path, b"before\n".to_vec(), Default::default(), None)
        .await?;
    test.codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "Run the authored streamed patch fixture.".to_owned(),
            text_elements: Vec::new(),
        }]))
        .await?;
    let mut events = collect_until(&test, |event| matches!(event, EventMsg::ItemCompleted(completed)
        if matches!(&completed.item, TurnItem::AgentMessage(item) if item.id == "stream-checkpoint"))).await?;
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, EventMsg::ItemStarted(started)
        if matches!(&started.item, TurnItem::SourceDescription(_))))
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, EventMsg::ItemCompleted(completed)
        if matches!(&completed.item, TurnItem::SourceDescription(_))))
    );
    assert!(!events.iter().any(is_patch_execution_event));
    assert_eq!(
        test.fs()
            .read_file_text(&file_path, Default::default(), None)
            .await?,
        "before\n"
    );
    release_final
        .send(())
        .expect("release final tool arguments");
    events.extend(collect_until(&test, |event| matches!(event, EventMsg::TurnComplete(_))).await?);
    let description = atomic_description(&events);
    assert_eq!(description["disposition"], "completeSyntax");
    assert_eq!(description["sources"][0]["text"], code);
    assert_native_patch_succeeded(&events);
    assert_eq!(
        test.fs()
            .read_file_text(&file_path, Default::default(), None)
            .await?,
        "after\n"
    );
    test.codex.shutdown_and_wait().await?;
    server.shutdown().await;
    Ok(())
}
