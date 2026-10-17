//! Existing session interfaces record the handoff without evaluating its source.

use super::super::attach_in_memory_thread_store;
use super::super::make_session_and_context_with_auth_and_config_and_rx;
use crate::config::Config;
use crate::function_tool::FunctionCallError;
use crate::session::session::Session;
use crate::session::step_context::StepContext;
use crate::tools::code_mode::CodeModeExecuteHandler;
use crate::tools::code_mode::CodeModeService;
use crate::tools::code_mode::execute_spec::create_code_mode_tool;
use crate::tools::context::ToolCallSource;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::tools::handlers::apply_patch_spec::create_apply_patch_freeform_tool;
use crate::tools::registry::ToolExecutor;
use crate::turn_diff_tracker::TurnDiffTracker;
use codex_code_mode::CodeModeSession;
use codex_code_mode::CodeModeSessionDelegate;
use codex_code_mode::CodeModeSessionProvider;
use codex_code_mode::CodeModeSessionProviderFuture;
use codex_code_mode::CodeModeSessionResultFuture;
use codex_code_mode::ExecuteRequest;
use codex_code_mode::ImageDetailVisibility;
use codex_code_mode::StartedCell;
use codex_file_system::EnvironmentAccess;
use codex_file_system::ReadFileOptions;
use codex_file_system::WriteFileOptions;
use codex_login::CodexAuth;
use codex_protocol::ThreadId;
use codex_protocol::items::TurnItem;
use codex_protocol::models::PermissionProfile;
use codex_protocol::protocol::Event;
use codex_rollout::RolloutItem;
use codex_thread_store::InMemoryThreadStore;
use codex_thread_store::LoadThreadHistoryParams;
use codex_thread_store::ThreadStore;
use codex_tools::JsonSchema;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use codex_utils_absolute_path::AbsolutePathBuf;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Mutex;
use tokio_util::sync::CancellationToken;

pub(super) const INERT_HANDOFF: &str = "authored recording runtime does not evaluate source";

pub(super) struct Observation {
    pub(super) request: ExecuteRequest,
    pub(super) events: Vec<Event>,
    pub(super) persisted: Vec<RolloutItem>,
    pub(super) preempt: Option<CancellationToken>,
}

struct RecordingRuntime {
    events: async_channel::Receiver<Event>,
    store: Arc<InMemoryThreadStore>,
    thread_id: ThreadId,
    observation: Mutex<Option<Observation>>,
}

struct RecordingProvider(Arc<RecordingRuntime>);

impl CodeModeSessionProvider for RecordingProvider {
    fn create_session(&self) -> CodeModeSessionProviderFuture<'_> {
        Box::pin(async { Ok(Arc::clone(&self.0) as Arc<dyn CodeModeSession>) })
    }
}

impl CodeModeSession for RecordingRuntime {
    fn execute<'a>(
        &'a self,
        request: ExecuteRequest,
        _delegate: Arc<dyn CodeModeSessionDelegate>,
        preempt: Option<CancellationToken>,
    ) -> CodeModeSessionResultFuture<'a, StartedCell> {
        Box::pin(async move {
            // Observe at the actual runtime boundary, before returning our error.
            let events = std::iter::from_fn(|| self.events.try_recv().ok()).collect();
            let persisted = self
                .store
                .load_history(LoadThreadHistoryParams {
                    thread_id: self.thread_id,
                    include_archived: false,
                })
                .await
                .expect("test thread history")
                .items;
            let previous = self
                .observation
                .lock()
                .expect("recording lock")
                .replace(Observation {
                    request,
                    events,
                    persisted,
                    preempt,
                });
            assert!(previous.is_none(), "one runtime request per test fixture");
            Err(INERT_HANDOFF.to_owned())
        })
    }

    fn wait<'a>(
        &'a self,
        _request: codex_code_mode::WaitRequest,
        _preempt: Option<CancellationToken>,
    ) -> CodeModeSessionResultFuture<'a, codex_code_mode::WaitOutcome> {
        Box::pin(async { panic!("recording fixture must not wait for a cell") })
    }

    fn terminate<'a>(
        &'a self,
        _cell_id: codex_code_mode::CellId,
    ) -> CodeModeSessionResultFuture<'a, codex_code_mode::WaitOutcome> {
        Box::pin(async { panic!("recording fixture must not create a cell") })
    }

    fn shutdown(&self) -> CodeModeSessionResultFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
}

pub(super) struct Fixture {
    _directory: tempfile::TempDir,
    pub(super) session: Arc<Session>,
    pub(super) step: Arc<StepContext>,
    runtime: Arc<RecordingRuntime>,
    handler: CodeModeExecuteHandler,
}

impl Fixture {
    pub(super) async fn new() -> Self {
        let directory = tempfile::tempdir().expect("disposable publication fixture");
        let (mut session, turn, events) = make_session_and_context_with_auth_and_config_and_rx(
            CodexAuth::from_api_key("Test API Key"),
            Vec::new(),
            |config| {
                config.cwd =
                    AbsolutePathBuf::from_absolute_path(directory.path()).expect("fixture cwd");
                config
                    .permissions
                    .set_permission_profile(PermissionProfile::Disabled)
                    .expect("test-owned filesystem permission profile");
            },
        )
        .await;
        let environment = turn.initial_environments.primary().expect("local fixture");
        environment
            .fs_accessor(None)
            .write_file(
                &environment.cwd().join("file").expect("fixture path"),
                b"old".to_vec(),
                WriteFileOptions::default(),
            )
            .await
            .expect("authored entry bytes");
        let mutable = Arc::get_mut(&mut session).expect("unshared test session");
        let store = attach_in_memory_thread_store(mutable).await;
        let runtime = Arc::new(RecordingRuntime {
            events,
            store,
            thread_id: mutable.thread_id,
            observation: Mutex::new(None),
        });
        mutable.services.code_mode_service = CodeModeService::new(
            mutable.thread_id,
            Arc::new(RecordingProvider(Arc::clone(&runtime))),
            &turn.config.code_mode,
            mutable.services.executed_tool_calls.clone(),
        );
        // Direct handler calls bypass RegularTask's turn-start publication.
        session.emit_turn_started(&turn).await;
        let mut step = StepContext::for_test(turn);
        Arc::get_mut(&mut step).expect("unshared step").preempt = Some(CancellationToken::new());
        let exec_command = ToolSpec::Function(ResponsesApiTool {
            name: "exec_command".to_owned(),
            description: "Authored inert nested tool definition".to_owned(),
            strict: false,
            defer_loading: None,
            parameters: JsonSchema::default(),
            output_schema: None,
        });
        let handler = CodeModeExecuteHandler::new(
            create_code_mode_tool(
                &[],
                &[],
                &BTreeMap::new(),
                codex_code_mode::DEFAULT_EXEC_YIELD_TIME_MS,
                true,
                ImageDetailVisibility::Visible,
                None,
            ),
            vec![
                (Arc::new(create_apply_patch_freeform_tool(false)), None),
                (Arc::new(exec_command), None),
            ],
        );
        Self {
            _directory: directory,
            session,
            step,
            runtime,
            handler,
        }
    }

    pub(super) async fn invoke(&self, source: &str, cancellation: CancellationToken) -> String {
        let invocation = ToolInvocation {
            session: Arc::clone(&self.session),
            turn: Arc::clone(&self.step.turn),
            step_context: Arc::clone(&self.step),
            cancellation_token: cancellation,
            tracker: Arc::new(tokio::sync::Mutex::new(TurnDiffTracker::new())),
            call_id: "publication-call".to_owned(),
            tool_name: ToolName::plain("exec"),
            source: ToolCallSource::Direct,
            payload: ToolPayload::Custom {
                input: source.to_owned(),
            },
        };
        match self.handler.handle(invocation).await {
            Err(FunctionCallError::RespondToModel(message)) => message,
            Err(error) => panic!("unexpected handler error: {error:?}"),
            Ok(_) => panic!("inert recording runtime cannot complete execution"),
        }
    }

    pub(super) fn take_observation(&self) -> Option<Observation> {
        self.runtime
            .observation
            .lock()
            .expect("recording lock")
            .take()
    }

    pub(super) fn remaining_events(&self) -> Vec<Event> {
        std::iter::from_fn(|| self.runtime.events.try_recv().ok()).collect()
    }

    pub(super) async fn persisted(&self) -> Vec<RolloutItem> {
        self.session
            .live_thread()
            .expect("test persistence")
            .load_history(false)
            .await
            .expect("test history")
            .items
    }

    pub(super) async fn bytes(&self) -> Vec<u8> {
        let environment = self.step.environments.primary().expect("local fixture");
        environment
            .fs_accessor(None)
            .read_file(
                &environment.cwd().join("file").expect("fixture path"),
                ReadFileOptions::default(),
            )
            .await
            .expect("unchanged fixture file")
    }

    pub(super) fn cancel_during_source_completion(&mut self, cancellation: CancellationToken) {
        let mut extensions = codex_extension_api::ExtensionRegistryBuilder::<Config>::new();
        extensions.turn_lifecycle_contributor(Arc::new(CancelSourceCompletion(cancellation)));
        Arc::get_mut(&mut self.session)
            .expect("unshared session before invocation")
            .services
            .extensions = Arc::new(extensions.build());
    }
}

struct CancelSourceCompletion(CancellationToken);

impl codex_extension_api::TurnLifecycleContributor for CancelSourceCompletion {
    fn on_item_completed<'a>(
        &'a self,
        _thread_store: &'a codex_extension_api::ExtensionData,
        _turn_store: &'a codex_extension_api::ExtensionData,
        item: &'a TurnItem,
    ) -> codex_extension_api::ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if matches!(item, TurnItem::SourceDescription(_)) {
                self.0.cancel();
            }
        })
    }
}
