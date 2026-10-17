//! Admission and presentation happen before the JavaScript runtime is created.
//! The bounded wait includes blocking-pool scheduling. Cancellation drops late
//! results; it cannot preempt already-running native analysis or event delivery.

mod convert;
mod selective;
mod supplier;

use crate::function_tool::FunctionCallError;
use crate::session::session::Session;
use crate::session::step_context::StepContext;
use crate::session::turn_context::TurnContext;
use codex_code_activity::AnalysisLimits;
use codex_code_activity::CallId;
use codex_code_activity::CompleteSourceDescriptionInput;
use codex_code_activity::GUARD_MAX_CALL_ID_BYTES;
use codex_code_activity::GUARD_MAX_SOURCE_BYTES;
use codex_code_activity::GUARD_MAX_WAIT;
use codex_code_activity::Language;
use codex_code_activity::SourceAnalysisGuard;
use codex_code_activity::SourcePresentationInput;
use codex_code_mode::ToolDefinition;
use codex_protocol::items::TurnItem;
use codex_protocol::source_description::SourceDescription;
use codex_protocol::source_description::SourceDescriptionFallback;
use codex_protocol::source_description::SourceDescriptionItem;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Instant;
use tokio::sync::Semaphore;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

static ADMISSION: OnceLock<Arc<Semaphore>> = OnceLock::new();

// Same-version clients consume the coordinated source-description schema. This
// does not imply negotiation with older clients or alter runtime tool bindings.
pub(super) async fn publish(
    session: &Session,
    turn: &TurnContext,
    step: &StepContext,
    call_id: &str,
    source: &str,
    enabled_tools: &[ToolDefinition],
    cancellation: &CancellationToken,
) -> Result<(), FunctionCallError> {
    let admission = ADMISSION.get_or_init(|| Arc::new(Semaphore::new(1)));
    let description = describe(
        call_id,
        source,
        enabled_tools,
        cancellation,
        Arc::clone(admission),
        step.environments
            .primary()
            .cloned()
            .map(supplier::SnapshotSupplier::new),
    )
    .await?;
    check_cancelled(cancellation)?;
    let item = TurnItem::SourceDescription(SourceDescriptionItem {
        id: format!("source-{}", Uuid::new_v4()),
        call_id: call_id.to_owned(),
        description,
    });
    // Both events contain the same final analysis. Completion does not describe
    // execution, and publication order does not promise a remote client has painted.
    session.emit_turn_item_started(turn, &item).await;
    session.emit_turn_item_completed(turn, item).await;
    check_cancelled(cancellation)
}

async fn describe(
    call_id: &str,
    source: &str,
    enabled_tools: &[ToolDefinition],
    cancellation: &CancellationToken,
    admission: Arc<Semaphore>,
    supplier: Option<supplier::SnapshotSupplier>,
) -> Result<SourceDescription, FunctionCallError> {
    check_cancelled(cancellation)?;
    let deadline = Instant::now() + GUARD_MAX_WAIT;
    if call_id.len() > GUARD_MAX_CALL_ID_BYTES || source.len() > GUARD_MAX_SOURCE_BYTES {
        return Ok(fallback(SourceDescriptionFallback::SourceLimit));
    }
    let Ok(permit) = admission.try_acquire_owned() else {
        return Ok(fallback(SourceDescriptionFallback::Busy));
    };
    let input = CompleteSourceDescriptionInput::from_source(
        CallId::from(call_id),
        Language::TypeScript,
        source,
        AnalysisLimits::new(GUARD_MAX_SOURCE_BYTES, GUARD_MAX_WAIT),
    );
    let Ok(input) = input else {
        return Ok(fallback(SourceDescriptionFallback::SourceLimit));
    };
    // The outer cell has no single resolved child execution environment. Keep
    // its entry cwd unknown; literal child workdir arguments remain source data.
    let Ok(input) = SourcePresentationInput::new(input, selective::mode(enabled_tools), None)
    else {
        return Ok(fallback(SourceDescriptionFallback::SourceLimit));
    };
    let worker_cancellation = cancellation.child_token();
    let _cancel_worker_on_drop = worker_cancellation.clone().drop_guard();
    let runtime = tokio::runtime::Handle::current();
    let mut task = AbortOnDrop(tokio::task::spawn_blocking(move || {
        let _permit = permit;
        if worker_cancellation.is_cancelled() || Instant::now() >= deadline {
            return fallback(SourceDescriptionFallback::Deadline);
        }
        let guard = SourceAnalysisGuard::shared();
        if worker_cancellation.is_cancelled() {
            return fallback(SourceDescriptionFallback::Deadline);
        }
        let result = guard.present_until(input, deadline);
        let snapshots = match (
            supplier,
            result.as_ref().ok().and_then(|value| value.forecast()),
        ) {
            (Some(supplier), Some(analysis)) => {
                runtime.block_on(supplier.supply(analysis, &worker_cancellation, deadline))
            }
            _ => Vec::new(),
        };
        selective::description(result, &snapshots)
    }));
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err(cancelled()),
        _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => {
            Ok(fallback(SourceDescriptionFallback::Deadline))
        }
        result = &mut task.0 => Ok(match result {
            Ok(description) if Instant::now() < deadline => description,
            Ok(_) => fallback(SourceDescriptionFallback::Deadline),
            Err(_) => fallback(SourceDescriptionFallback::Unavailable),
        }),
    }
}

// Aborting cancels work still queued in Tokio's pool. A running task retains its
// permit until disposal, so a dropped/expired request cannot create another job.
struct AbortOnDrop(JoinHandle<SourceDescription>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn fallback(reason: SourceDescriptionFallback) -> SourceDescription {
    SourceDescription::Fallback { reason }
}

pub(super) fn check_cancelled(cancellation: &CancellationToken) -> Result<(), FunctionCallError> {
    if cancellation.is_cancelled() {
        Err(cancelled())
    } else {
        Ok(())
    }
}

fn cancelled() -> FunctionCallError {
    FunctionCallError::RespondToModel("Code execution was cancelled before starting.".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[tokio::test]
    async fn busy_gate_has_no_source_children() {
        let admission = Arc::new(Semaphore::new(0));
        let result = describe(
            "call",
            "secret()",
            &[],
            &CancellationToken::new(),
            admission,
            None,
        )
        .await
        .expect("busy is a presentation fallback");
        assert_eq!(result, fallback(SourceDescriptionFallback::Busy));
    }

    #[tokio::test]
    async fn cancelled_gate_never_publishes_or_admits() {
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let admission = Arc::new(Semaphore::new(1));
        assert!(
            describe(
                "call",
                "text()",
                &[],
                &cancellation,
                Arc::clone(&admission),
                None
            )
            .await
            .is_err()
        );
        assert_eq!(admission.available_permits(), 1);
    }
}
