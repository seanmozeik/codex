use super::DemoError;
use super::Result;
use super::input::Completion;
use super::input::Launch;
use codex_code_activity::ActivityStream;
use codex_code_activity::AnalysisLimits;
use codex_code_activity::CaptureError;
use codex_code_activity::CompleteSourceDescriptionInput;
use codex_code_activity::EnvironmentAssumptions;
use codex_code_activity::ExecutionResult;
use codex_code_activity::ExecutionStatus;
use codex_code_activity::FileState;
use codex_code_activity::Language;
use codex_code_activity::LocalFiles;
use codex_code_activity::PreparedCall;
use codex_code_activity::SnapshotProvider;
use codex_code_activity::SourceAnalysisGuard;
use codex_code_activity::analyze_guarded_codex_command;
use codex_protocol::ThreadId;
use codex_protocol::items::CommandExecutionItem;
use codex_protocol::items::CommandExecutionStatus;
use codex_protocol::items::TurnItem;
use codex_protocol::protocol::ExecCommandSource;
use codex_protocol::protocol::HasLegacyEvent;
use codex_protocol::protocol::ItemCompletedEvent;
use codex_protocol::protocol::ItemStartedEvent;
use codex_shell_command::parse_command::parse_command;
use codex_utils_path_uri::PathUri;
use serde_json::Value;
use serde_json::json;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

struct FixedFile {
    provider: LocalFiles,
    path: PathBuf,
}

impl SnapshotProvider for FixedFile {
    fn capture(&mut self, path: &Path, max_bytes: usize) -> FileState {
        if path != self.path {
            return FileState::Unavailable(CaptureError::OutsideRoot);
        }
        self.provider.capture(path, max_bytes)
    }
}

pub(super) struct Prepared {
    item: CommandExecutionItem,
    started_at_ms: i64,
    observations: PreparedCall,
    files: FixedFile,
}

pub(super) fn prepare(launch: Launch) -> Result<(Prepared, Value)> {
    launch.validate()?;
    let item = CommandExecutionItem {
        sandbox_type: None,
        model_context: None,
        id: launch.call_id,
        plugin_id: None,
        script_path: None,
        process_id: None,
        parsed_cmd: parse_command(&launch.command),
        command: launch.command,
        cwd: PathUri::from_host_native_path(&launch.cwd)?,
        source: ExecCommandSource::Agent,
        interaction_input: None,
        status: CommandExecutionStatus::InProgress,
        aggregated_output: None,
        exit_code: None,
        duration: None,
    };
    let strict = analyze_guarded_codex_command(
        &item,
        EnvironmentAssumptions::PersistentOrUnknown,
        AnalysisLimits::new(32768, Duration::from_millis(100)),
        Duration::from_millis(100),
    );
    if !strict.ui_rows().is_empty() {
        return Err(DemoError::Protocol(
            "unknown environment unexpectedly produced intent rows",
        ));
    }
    // This independent lane names complete source syntax only. It does not
    // promote unknown runtime bindings into effects or select snapshot targets.
    let source_input = CompleteSourceDescriptionInput::from_source(
        item.id.as_str().into(),
        Language::Shell,
        &item.command[2],
        AnalysisLimits::new(32768, Duration::from_millis(100)),
    )?;
    let source_description =
        SourceAnalysisGuard::shared().describe(source_input, Duration::from_millis(100));
    let capture_root = Path::new(&launch.cwd).canonicalize()?;
    let provider = LocalFiles::new(&capture_root)?;
    let path = capture_root.join("settings.txt");
    let mut files = FixedFile { provider, path };
    // This empty observation stream is not an analysis of the command. Reuse
    // only the lifecycle's host-selected endpoint capture; the guarded bridge
    // above handles the real source. No source-derived paths or IDs are present.
    let stream = ActivityStream::new(item.id.as_str().into(), Language::Shell, None)?;
    let capture_path = files.path.clone();
    let observations = stream.finish()?.prepare(&mut files, &[capture_path]);
    if !observations
        .activity()
        .files
        .iter()
        .any(|file| file.path == files.path)
    {
        return Err(DemoError::Protocol("fixed observation path was omitted"));
    }
    let event = ItemStartedEvent {
        thread_id: ThreadId::from_u128(1),
        turn_id: item.id.clone(),
        item: TurnItem::CommandExecution(item.clone()),
        started_at_ms: launch.started_at_ms,
    };
    let output = json!({
        "phase": "prepared", "itemStarted": event,
        "legacyEvents": event.as_legacy_events(false), "strict": strict,
        "rows": strict.ui_rows(), "originalCommand": item.command, "cwd": launch.cwd,
        "sourceDescription": { "label": "Source references", "analysis": source_description },
    });
    Ok((
        Prepared {
            item,
            started_at_ms: launch.started_at_ms,
            observations,
            files,
        },
        output,
    ))
}

impl Prepared {
    pub(super) fn complete(mut self, completion: Completion) -> Result<Value> {
        completion.validate(self.started_at_ms)?;
        let result = self.observations.complete(
            &mut self.files,
            ExecutionResult::new(
                ExecutionStatus::Exited {
                    code: completion.exit_code,
                },
                &completion.output,
            ),
        );
        self.item.status = if completion.exit_code == 0 {
            CommandExecutionStatus::Completed
        } else {
            CommandExecutionStatus::Failed
        };
        self.item.aggregated_output = Some(completion.output);
        self.item.exit_code = Some(completion.exit_code);
        self.item.duration = Some(Duration::from_micros(completion.duration_micros));
        let event = ItemCompletedEvent {
            thread_id: ThreadId::from_u128(1),
            turn_id: self.item.id.clone(),
            item: TurnItem::CommandExecution(self.item),
            started_at_ms: Some(self.started_at_ms),
            completed_at_ms: completion.completed_at_ms,
        };
        let observed: Vec<_> = result
            .files
            .iter()
            .filter(|file| file.path == self.files.path)
            .map(|file| {
                json!({ "path": file.path, "before": file.before, "after": file.after,
                "change": file.change, "diff": file.diff })
            })
            .collect();
        Ok(json!({
            "phase": "completed", "itemCompleted": event,
            "legacyEvents": event.as_legacy_events(false), "strictRows": [],
            "observed": { "basis": result.observation_basis, "files": observed },
        }))
    }
}
