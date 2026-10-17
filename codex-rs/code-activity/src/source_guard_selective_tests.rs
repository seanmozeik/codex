//! Typed-job regressions use a mock worker; fixture programs are never executed.

use super::GUARD_MAX_WAIT;
use super::Gate;
use super::SourceAnalysisGuard;
use super::State;
use super::initialize_and_budget;
use super::worker::DescriptionResponse;
use super::worker::EnvelopeResponse;
use super::worker::ForecastResponse;
use super::worker::Job;
use super::worker::Response;
use super::worker::Worker;
use crate::AnalysisLimits;
use crate::CompleteOrchestration;
use crate::CompleteScript;
use crate::CompleteSourceDescriptionInput;
use crate::GuardFallback;
use crate::Language;
use crate::SelectiveEditAnalysis;
use crate::SelectiveEditDisposition;
use crate::SelectiveEditInput;
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

fn input(call_id: &str) -> Result<SelectiveEditInput, crate::ActivityError> {
    SelectiveEditInput::from_source(
        call_id.into(),
        Language::Python,
        "inert()",
        Some("/workspace"),
        AnalysisLimits::new(4096, WAIT),
    )
}

struct ForecastWorker {
    block: Option<(SyncSender<()>, Receiver<()>)>,
}

impl Worker for ForecastWorker {
    fn forecast(&mut self, input: SelectiveEditInput, _: Instant) -> ForecastResponse {
        if let Some((started, release)) = self.block.take() {
            started
                .send(())
                .map_err(|_| GuardFallback::WorkerUnavailable)?;
            release
                .recv_timeout(WAIT)
                .map_err(|_| GuardFallback::WorkerUnavailable)?;
        }
        Ok(SelectiveEditAnalysis::empty(
            input.call_id,
            SelectiveEditDisposition::Analyzed,
        ))
    }

    fn description(
        &mut self,
        _: CompleteSourceDescriptionInput,
        _: Instant,
    ) -> DescriptionResponse {
        Err(GuardFallback::Deadline)
    }

    fn script(&mut self, _: CompleteScript, _: Instant) -> Response {
        Err(GuardFallback::Deadline)
    }

    fn orchestration(&mut self, _: CompleteOrchestration, _: Instant) -> EnvelopeResponse {
        Err(GuardFallback::Deadline)
    }
}

#[test]
fn expired_forecast_never_enqueues_or_acquires_admission() -> TestResult {
    let base = Instant::now();
    let (jobs, queued) = sync_channel(1);
    let guard = SourceAnalysisGuard {
        jobs,
        gate: Arc::new(Gate(AtomicU8::new(State::Ready as u8))),
    };
    let result = guard.forecast_until_with(
        input("expired")?,
        base,
        || base,
        |_, _| panic!("expired requests cannot wait"),
    );
    assert_eq!(result.err(), Some(GuardFallback::Deadline));
    assert!(matches!(queued.try_recv(), Err(TryRecvError::Empty)));
    assert_eq!(guard.gate.0.load(Ordering::Acquire), State::Ready as u8);
    Ok(())
}

#[test]
fn forecast_uses_original_deadline_including_host_delay() -> TestResult {
    let base = Instant::now();
    let (jobs, queued) = sync_channel(1);
    let guard = SourceAnalysisGuard {
        jobs,
        gate: Arc::new(Gate(AtomicU8::new(State::Ready as u8))),
    };
    let mut calls = 0;
    let result = guard
        .forecast_until_with(
            input("current")?,
            base + GUARD_MAX_WAIT,
            || {
                calls += 1;
                base + Duration::from_millis(match calls {
                    1 => 80,
                    2 => 90,
                    _ => 95,
                })
            },
            |receiver, remaining| {
                assert_eq!(remaining, Duration::from_millis(10));
                let Job::Forecast {
                    input,
                    deadline,
                    reply,
                } = queued
                    .try_recv()
                    .map_err(|_| RecvTimeoutError::Disconnected)?
                else {
                    panic!("typed forecast job");
                };
                assert_eq!(deadline, base + GUARD_MAX_WAIT);
                guard.finish_test_reply(
                    reply,
                    Ok(SelectiveEditAnalysis::empty(
                        input.call_id,
                        SelectiveEditDisposition::Analyzed,
                    )),
                );
                receiver.recv_timeout(WAIT)
            },
        )
        .map_err(|_| "forecast unexpectedly failed")?;
    assert_eq!(result.call_id.as_str(), "current");
    assert_eq!(calls, 3);
    Ok(())
}

#[test]
fn timed_out_forecast_holds_shared_gate_until_late_reply_disposal() -> TestResult {
    let base = Instant::now();
    let (started, began) = sync_channel(1);
    let (release, released) = sync_channel(1);
    let (retired, finished) = sync_channel(2);
    let guard = SourceAnalysisGuard::with_job_worker(
        ForecastWorker {
            block: Some((started, released)),
        },
        move || {
            let _ = retired.try_send(());
        },
        move || base,
    );
    let result = guard.forecast_until_with(
        input("stale")?,
        base + GUARD_MAX_WAIT,
        || base,
        |_, _| {
            began
                .recv_timeout(WAIT)
                .map_err(|_| RecvTimeoutError::Disconnected)?;
            Err(RecvTimeoutError::Timeout)
        },
    );
    assert_eq!(result.err(), Some(GuardFallback::Deadline));
    assert_eq!(
        guard
            .forecast_until_with(
                input("blocked")?,
                base + GUARD_MAX_WAIT,
                || base,
                |_, _| panic!("busy cannot receive")
            )
            .err(),
        Some(GuardFallback::Busy)
    );
    let description = CompleteSourceDescriptionInput::from_source(
        "description".into(),
        Language::Python,
        "inert()",
        AnalysisLimits::new(4096, WAIT),
    )?;
    assert_eq!(
        guard.describe(description, GUARD_MAX_WAIT).disposition(),
        SourceDescriptionGuardDisposition::Fallback(GuardFallback::Busy)
    );
    release.send(())?;
    finished.recv_timeout(WAIT)?;
    let fresh = guard
        .forecast_until_with(
            input("fresh")?,
            base + GUARD_MAX_WAIT,
            || base,
            |receiver, _| receiver.recv_timeout(WAIT),
        )
        .map_err(|_| "fresh forecast failed")?;
    finished.recv_timeout(WAIT)?;
    assert_eq!(fresh.call_id.as_str(), "fresh");
    assert!(!serde_json::to_string(&fresh)?.contains("stale"));
    Ok(())
}

#[test]
fn setup_reduces_forecast_budget_without_mutating_source_or_cwd() -> TestResult {
    let base = Instant::now();
    let mut analyzer = None;
    let mut calls = 0;
    let result = initialize_and_budget(
        input("budget")?,
        base + GUARD_MAX_WAIT,
        &mut analyzer,
        || Some(()),
        || {
            calls += 1;
            base + Duration::from_millis(if calls == 1 { 20 } else { 80 })
        },
    )
    .map_err(|_| "budget rejected")?;
    assert_eq!(result.limits.elapsed(), Duration::from_millis(20));
    assert_eq!(result.limits.source_bytes(), 4096);
    assert_eq!(result.source(), "inert()");
    assert_eq!(result.cwd.as_deref(), Some("/workspace"));
    Ok(())
}
