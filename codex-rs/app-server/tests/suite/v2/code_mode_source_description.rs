//! Real code-mode source items survive app-server transport and persisted history.

use std::error::Error;
use std::time::Duration;

use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use codex_app_server_protocol::FsReadFileParams;
use codex_app_server_protocol::FsReadFileResponse;
use codex_app_server_protocol::FsWriteFileParams;
use codex_app_server_protocol::FsWriteFileResponse;
use codex_app_server_protocol::ItemCompletedNotification;
use codex_app_server_protocol::ItemStartedNotification;
use codex_app_server_protocol::JSONRPCMessage;
use codex_app_server_protocol::ThreadReadParams;
use codex_app_server_protocol::ThreadReadResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use codex_utils_absolute_path::AbsolutePathBuf;
use core_test_support::responses;
use core_test_support::skip_if_no_network;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;
use test_case::test_case;
use tokio::time::timeout;

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

const READ_TIMEOUT: Duration = Duration::from_secs(60);
const CALL_ID: &str = "persisted-source-call";
const FILE_NAME: &str = "source-history-fixture.txt";
const PATCH: &str = "*** Begin Patch\n*** Update File: source-history-fixture.txt\n@@\n-before\n+after\n*** End Patch\n";

#[derive(Clone, Copy)]
enum PatchCase {
    MatchingContext,
    MismatchingContext,
    UnsupportedSibling,
}

#[derive(Default)]
struct LiveItems {
    source_started: Vec<Value>,
    source_completed: Vec<Value>,
    files_completed: Vec<Value>,
    diffs: Vec<String>,
}

async fn collect_live_items(app: &mut TestAppServer) -> TestResult<LiveItems> {
    timeout(READ_TIMEOUT, async {
        let mut items = LiveItems::default();
        loop {
            let message = app.read_next_message().await?;
            let JSONRPCMessage::Notification(notification) = message else {
                assert!(!matches!(message, JSONRPCMessage::Request(_)), "authored fixture unexpectedly requested approval: {message:?}");
                continue;
            };
            let params = notification.params.expect("notification parameters");
            match notification.method.as_str() {
                "item/started" => {
                    let _: ItemStartedNotification = serde_json::from_value(params.clone())?;
                    match params["item"]["type"].as_str() {
                        Some("sourceDescription") => items.source_started.push(params["item"].clone()),
                        Some("fileChange") => assert_eq!(items.source_completed.len(), 1, "submitted-source completion must precede the real nested file lifecycle"),
                        _ => {}
                    }
                }
                "item/completed" => {
                    let _: ItemCompletedNotification = serde_json::from_value(params.clone())?;
                    match params["item"]["type"].as_str() {
                        Some("sourceDescription") => items.source_completed.push(params["item"].clone()),
                        Some("fileChange") => {
                            assert_eq!(items.source_completed.len(), 1);
                            items.files_completed.push(params["item"].clone());
                        }
                        _ => {}
                    }
                }
                "turn/diff/updated" => {
                    assert_eq!(items.source_completed.len(), 1);
                    items.diffs.push(params["diff"].as_str().expect("native turn diff").to_owned());
                }
                "turn/completed" => {
                    let completed: TurnCompletedNotification = serde_json::from_value(params)?;
                    assert_eq!(completed.turn.status, TurnStatus::Completed);
                    return Ok(items);
                }
                _ => {}
            }
        }
    })
    .await?
}

async fn read_history_items(app: &mut TestAppServer, thread_id: &str) -> TestResult<Vec<Value>> {
    let request_id = app
        .send_thread_read_request(ThreadReadParams {
            thread_id: thread_id.to_owned(),
            include_turns: true,
        })
        .await?;
    let response: ThreadReadResponse =
        timeout(READ_TIMEOUT, app.read_response(request_id)).await??;
    response
        .thread
        .turns
        .iter()
        .flat_map(|turn| &turn.items)
        .map(|item| serde_json::to_value(item).map_err(Into::into))
        .collect()
}

fn assert_history_matches_live(items: &[Value], live: &LiveItems) {
    let sources = items
        .iter()
        .filter(|item| item["type"] == "sourceDescription")
        .cloned()
        .collect::<Vec<_>>();
    let files = items
        .iter()
        .filter(|item| item["type"] == "fileChange")
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        sources, live.source_completed,
        "history must keep exactly the completed atomic source item"
    );
    assert_eq!(
        files, live.files_completed,
        "source persistence must not invent or duplicate applied-file items"
    );
}

#[test_case(PatchCase::MatchingContext; "matching_file_context")]
#[test_case(PatchCase::MismatchingContext; "mismatching_file_context")]
#[test_case(PatchCase::UnsupportedSibling; "unsupported_late_sibling")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn code_mode_source_items_keep_live_order_and_durable_history(case: PatchCase) -> TestResult {
    skip_if_no_network!(Ok(()));
    let server = responses::start_mock_server().await;
    let mut code = format!(
        "text(await tools.apply_patch({}));",
        serde_json::to_string(PATCH)?
    );
    if matches!(case, PatchCase::UnsupportedSibling) {
        code.push_str("\nMath.max(1, 2);");
    }
    let model_requests = responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse(vec![
                responses::ev_response_created("source-model-response"),
                responses::ev_custom_tool_call(CALL_ID, "exec", &code),
                responses::ev_completed("source-model-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("source-model-done", "done"),
                responses::ev_completed("source-model-follow-up"),
            ]),
        ],
    )
    .await;
    let codex_home = TempDir::new()?;
    let workspace = TempDir::new()?;
    let fixture_path = AbsolutePathBuf::try_from(workspace.path().join(FILE_NAME))?;
    let initial = if matches!(case, PatchCase::MismatchingContext) {
        "different file contents\n"
    } else {
        "before\n"
    };
    MockResponsesConfig::new(&server.uri())
        .enable_feature(Feature::CodeModeOnly)
        .enable_feature(Feature::CodeModeHost)
        .disable_feature(Feature::ShellSnapshot)
        .with_sandbox_mode("workspace-write")
        .write(codex_home.path())?;
    let mut app = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .without_auto_env()
        .build_initialized()
        .await?;
    let write_id = app
        .send_fs_write_file_request(FsWriteFileParams {
            path: fixture_path.clone(),
            data_base64: STANDARD.encode(initial),
        })
        .await?;
    let _: FsWriteFileResponse = timeout(READ_TIMEOUT, app.read_response(write_id)).await??;
    let thread = app
        .start_thread(ThreadStartParams {
            cwd: Some(workspace.path().to_string_lossy().into_owned()),
            ..Default::default()
        })
        .await?;
    let thread_id = thread.thread.id;
    let turn_id = app
        .send_turn_start_request(TurnStartParams {
            thread_id: thread_id.clone(),
            input: vec![UserInput::Text {
                text: "Run the authored patch fixture.".to_owned(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    let _: TurnStartResponse = timeout(READ_TIMEOUT, app.read_response(turn_id)).await??;
    let live = collect_live_items(&mut app).await?;
    assert_eq!(live.source_started.len(), 1);
    assert_eq!(live.source_completed, live.source_started);
    let source = &live.source_completed[0];
    assert_eq!(source["callId"], CALL_ID);
    assert_ne!(source["id"], CALL_ID);
    if matches!(case, PatchCase::UnsupportedSibling) {
        assert_eq!(
            source["description"],
            json!({"disposition": "fallback", "reason": "unsupportedSource"})
        );
    } else {
        assert_eq!(source["description"]["disposition"], "completeSyntax");
        assert_eq!(source["description"]["sources"][0]["text"], code);
        assert_eq!(source["description"]["patches"][0]["payload"], PATCH);
    }
    if matches!(case, PatchCase::MismatchingContext) {
        assert!(
            live.diffs.is_empty(),
            "submitted hunks must not become an applied diff after validation failure"
        );
        assert!(
            live.files_completed
                .iter()
                .all(|item| item["status"] != "completed")
        );
    } else {
        assert_eq!(live.files_completed.len(), 1);
        assert_eq!(live.files_completed[0]["status"], "completed");
        assert_eq!(
            live.files_completed[0]["changes"]
                .as_array()
                .expect("real file changes")
                .len(),
            1
        );
        assert_ne!(live.files_completed[0]["id"], source["id"]);
        assert!(
            live.diffs
                .iter()
                .any(|diff| diff.contains("-before") && diff.contains("+after"))
        );
    }
    assert_history_matches_live(&read_history_items(&mut app, &thread_id).await?, &live);
    assert!(
        timeout(READ_TIMEOUT, app.shutdown_gracefully())
            .await??
            .success()
    );

    let mut replay = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .without_auto_env()
        .build_initialized()
        .await?;
    assert_history_matches_live(&read_history_items(&mut replay, &thread_id).await?, &live);
    let read_id = replay
        .send_fs_read_file_request(FsReadFileParams { path: fixture_path })
        .await?;
    let contents: FsReadFileResponse =
        timeout(READ_TIMEOUT, replay.read_response(read_id)).await??;
    let expected = if matches!(case, PatchCase::MismatchingContext) {
        initial
    } else {
        "after\n"
    };
    assert_eq!(STANDARD.decode(contents.data_base64)?, expected.as_bytes());
    assert_eq!(
        model_requests.requests().len(),
        2,
        "history replay must not resample or execute the submitted source"
    );
    assert!(
        timeout(READ_TIMEOUT, replay.shutdown_gracefully())
            .await??
            .success()
    );
    Ok(())
}
