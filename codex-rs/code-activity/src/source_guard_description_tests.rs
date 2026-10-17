//! Cross-kind admission tests; all fixture source is parsed, never executed.

#[path = "source_guard_description_tests/deadlines_and_effects.rs"]
mod deadlines_and_effects;

use super::GUARD_MAX_WAIT;
use super::Gate;
use super::SourceAnalysisGuard;
use super::State;
use super::initialize_and_budget;
use super::worker::DescriptionResponse;
use super::worker::EnvelopeResponse;
use super::worker::Job;
use super::worker::Response;
use super::worker::Worker;
use crate::AnalysisLimits;
use crate::Analyzer;
use crate::CompleteOrchestration;
use crate::CompleteScript;
use crate::CompleteSourceDescriptionInput;
use crate::EnvironmentAssumptions;
use crate::GuardDisposition;
use crate::GuardFallback;
use crate::Language;
use crate::OrchestratorBindings;
use crate::Request;
use crate::SourceDescription;
use crate::SourceDescriptionGuardDisposition;
use pretty_assertions::assert_eq;
use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::AtomicU8;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::mpsc::SyncSender;
use std::sync::mpsc::TryRecvError;
use std::sync::mpsc::sync_channel;
use std::time::Duration;
use std::time::Instant;

type TestResult = Result<(), Box<dyn Error>>;
const WAIT: Duration = Duration::from_secs(5);
const SOURCE: &str = "from pathlib import Path\np = Path('a')\np.read_text()";

#[derive(Clone, Copy, PartialEq, Eq)]
enum BlockKind {
    Description,
    Script,
}

struct TestWorker {
    analyzer: Analyzer,
    block: Option<(BlockKind, SyncSender<()>, Receiver<()>)>,
}

impl TestWorker {
    fn pause(&mut self, kind: BlockKind) -> Result<(), GuardFallback> {
        if self
            .block
            .as_ref()
            .is_some_and(|(target, _, _)| *target == kind)
        {
            let (_, started, release) =
                self.block.take().ok_or(GuardFallback::WorkerUnavailable)?;
            started
                .send(())
                .map_err(|_| GuardFallback::WorkerUnavailable)?;
            release
                .recv_timeout(WAIT)
                .map_err(|_| GuardFallback::WorkerUnavailable)?;
        }
        Ok(())
    }
}

impl Worker for TestWorker {
    fn description(
        &mut self,
        input: CompleteSourceDescriptionInput,
        _: Instant,
    ) -> DescriptionResponse {
        self.pause(BlockKind::Description)?;
        Ok(self.analyzer.describe_complete(input))
    }

    fn script(&mut self, input: CompleteScript, _: Instant) -> Response {
        self.pause(BlockKind::Script)?;
        Ok(self.analyzer.analyze_complete(input))
    }

    fn orchestration(&mut self, input: CompleteOrchestration, _: Instant) -> EnvelopeResponse {
        Ok(self.analyzer.analyze_orchestration(input))
    }
}

fn description(source: &str) -> Result<CompleteSourceDescriptionInput, crate::ActivityError> {
    CompleteSourceDescriptionInput::from_source(
        "reused-id".into(),
        Language::Python,
        source,
        AnalysisLimits::new(4096, WAIT),
    )
}

fn script(environment: EnvironmentAssumptions) -> Result<CompleteScript, crate::ActivityError> {
    CompleteScript::from_request(
        Request::new("reused-id".into(), Language::Python, SOURCE.into(), None),
        environment,
        AnalysisLimits::new(4096, WAIT),
    )
}

fn envelope() -> Result<CompleteOrchestration, crate::OrchestrationInputError> {
    CompleteOrchestration::from_source(
        "reused-id".into(),
        r#"await tools.exec_command({cmd: "python -c \"open('a').read()\""});"#,
        OrchestratorBindings::TrustedFreshHostBindings,
        EnvironmentAssumptions::FreshProcess,
        AnalysisLimits::new(4096, WAIT),
    )
}

#[test]
fn timed_out_description_retains_shared_admission_and_discards_its_late_reply() -> TestResult {
    let base = Instant::now();
    let (started, began) = sync_channel(1);
    let (release, released) = sync_channel(1);
    let (retired, finished) = sync_channel(2);
    let guard = SourceAnalysisGuard::with_job_worker(
        TestWorker {
            analyzer: Analyzer::new()?,
            block: Some((BlockKind::Description, started, released)),
        },
        move || {
            let _ = retired.try_send(());
        },
        move || base,
    );
    let stale = description("open('stale').read()")?;
    let result = guard.exchange(
        GUARD_MAX_WAIT,
        || base,
        |_, _| {
            began
                .recv_timeout(WAIT)
                .map_err(|_| RecvTimeoutError::Disconnected)?;
            Err(RecvTimeoutError::Timeout)
        },
        |deadline, reply| Job::Description {
            input: stale,
            deadline,
            reply,
        },
    );
    assert_eq!(result.err(), Some(GuardFallback::Deadline));
    let blocked = guard.describe(description(SOURCE)?, GUARD_MAX_WAIT);
    assert_eq!(
        blocked.disposition(),
        SourceDescriptionGuardDisposition::Fallback(GuardFallback::Busy)
    );
    assert!(blocked.complete_description().is_none());
    assert_eq!(
        guard
            .analyze(
                script(EnvironmentAssumptions::FreshProcess)?,
                GUARD_MAX_WAIT
            )
            .disposition(),
        GuardDisposition::Fallback(GuardFallback::Busy)
    );
    assert_eq!(
        guard
            .analyze_orchestration(envelope()?, GUARD_MAX_WAIT)
            .disposition(),
        GuardDisposition::Fallback(GuardFallback::Busy)
    );
    release.send(())?;
    finished.recv_timeout(WAIT)?;
    let fresh = description("open('fresh').read()")?;
    let result = guard
        .exchange(
            GUARD_MAX_WAIT,
            || base,
            |receiver, _| receiver.recv_timeout(WAIT),
            |deadline, reply| Job::Description {
                input: fresh,
                deadline,
                reply,
            },
        )
        .map_err(|_| "fresh description failed")?;
    finished.recv_timeout(WAIT)?;
    assert!(
        result
            .references()
            .iter()
            .any(|r| r.literal.as_deref() == Some("fresh"))
    );
    assert!(!serde_json::to_string(&result)?.contains("stale"));
    Ok(())
}

#[test]
fn running_script_keeps_description_busy_until_cleanup() -> TestResult {
    let base = Instant::now();
    let (started, began) = sync_channel(1);
    let (release, released) = sync_channel(1);
    let (retired, finished) = sync_channel(1);
    let guard = SourceAnalysisGuard::with_job_worker(
        TestWorker {
            analyzer: Analyzer::new()?,
            block: Some((BlockKind::Script, started, released)),
        },
        move || {
            let _ = retired.try_send(());
        },
        move || base,
    );
    let result = guard.analyze_with(
        script(EnvironmentAssumptions::FreshProcess)?,
        GUARD_MAX_WAIT,
        || base,
        |_, _| {
            began
                .recv_timeout(WAIT)
                .map_err(|_| RecvTimeoutError::Disconnected)?;
            Err(RecvTimeoutError::Timeout)
        },
    );
    assert_eq!(
        result.disposition(),
        GuardDisposition::Fallback(GuardFallback::Deadline)
    );
    let blocked = guard.describe(description(SOURCE)?, GUARD_MAX_WAIT);
    assert_eq!(
        blocked.disposition(),
        SourceDescriptionGuardDisposition::Fallback(GuardFallback::Busy)
    );
    assert!(blocked.complete_description().is_none());
    release.send(())?;
    finished.recv_timeout(WAIT)?;
    Ok(())
}

#[test]
fn zero_wait_has_no_description_and_does_not_take_admission() -> TestResult {
    let base = Instant::now();
    let guard = SourceAnalysisGuard::with_job_worker(
        TestWorker {
            analyzer: Analyzer::new()?,
            block: None,
        },
        || {},
        move || base,
    );
    let absent = guard.describe(description(SOURCE)?, Duration::ZERO);
    assert_eq!(
        absent.disposition(),
        SourceDescriptionGuardDisposition::Fallback(GuardFallback::Deadline)
    );
    assert!(absent.complete_description().is_none());
    let present = guard.describe(description(SOURCE)?, GUARD_MAX_WAIT);
    assert!(present.complete_description().is_some());
    Ok(())
}
