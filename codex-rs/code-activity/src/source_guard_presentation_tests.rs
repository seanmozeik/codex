//! Combined-job lifecycle tests use injected workers and authored inert source.

use super::GUARD_MAX_WAIT;
use super::Gate;
use super::SourceAnalysisGuard;
use super::SourcePresentationInput;
use super::SourcePresentationMode;
use super::State;
use super::worker::DescriptionResponse;
use super::worker::EnvelopeResponse;
use super::worker::ForecastResponse;
use super::worker::Response;
use super::worker::Worker;
use crate::AnalysisLimits;
use crate::Analyzer;
use crate::CompleteOrchestration;
use crate::CompleteScript;
use crate::CompleteSourceDescriptionInput;
use crate::GuardFallback;
use crate::Language;
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
const SOURCE: &str = "open('note.txt', 'w').write('new')\n";

fn input(mode: SourcePresentationMode) -> Result<SourcePresentationInput, crate::ActivityError> {
    SourcePresentationInput::new(
        CompleteSourceDescriptionInput::from_source(
            "same-call".into(),
            Language::Python,
            SOURCE,
            AnalysisLimits::new(4096, WAIT),
        )?,
        mode,
        Some("/workspace"),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Forecast,
    Description,
}

struct PausedWorker {
    analyzer: Analyzer,
    phases: SyncSender<(Phase, Instant, String)>,
    release: Receiver<()>,
}

impl PausedWorker {
    fn pause(&self, phase: Phase, deadline: Instant, source: &str) -> Result<(), GuardFallback> {
        self.phases
            .send((phase, deadline, source.to_owned()))
            .map_err(|_| GuardFallback::WorkerUnavailable)?;
        self.release
            .recv_timeout(WAIT)
            .map_err(|_| GuardFallback::WorkerUnavailable)
    }
}

impl Worker for PausedWorker {
    fn forecast(&mut self, input: SelectiveEditInput, deadline: Instant) -> ForecastResponse {
        self.pause(Phase::Forecast, deadline, input.source())?;
        Ok(self.analyzer.forecast_edits(input))
    }
    fn description(
        &mut self,
        input: CompleteSourceDescriptionInput,
        deadline: Instant,
    ) -> DescriptionResponse {
        self.pause(Phase::Description, deadline, input.source())?;
        Ok(self.analyzer.describe_complete(input))
    }
    fn script(&mut self, _: CompleteScript, _: Instant) -> Response {
        Err(GuardFallback::WorkerUnavailable)
    }
    fn orchestration(&mut self, _: CompleteOrchestration, _: Instant) -> EnvelopeResponse {
        Err(GuardFallback::WorkerUnavailable)
    }
}

#[test]
fn combined_candidates_keep_one_admission_source_and_deadline() -> TestResult {
    let base = Instant::now();
    let (phase_tx, phases) = sync_channel(1);
    let (release, release_rx) = sync_channel(1);
    let (finished_tx, finished) = sync_channel(1);
    let guard = SourceAnalysisGuard::with_job_worker(
        PausedWorker {
            analyzer: Analyzer::new()?,
            phases: phase_tx,
            release: release_rx,
        },
        move || {
            let _ = finished_tx.try_send(());
        },
        move || base,
    );
    let result = guard
        .present_until_with(
            input(SourcePresentationMode::ConditionalForecast)?,
            base + GUARD_MAX_WAIT,
            || base,
            |receiver, remaining| {
                assert_eq!(remaining, GUARD_MAX_WAIT);
                for expected in [Phase::Forecast, Phase::Description] {
                    let (phase, deadline, source) = phases
                        .recv_timeout(WAIT)
                        .map_err(|_| RecvTimeoutError::Disconnected)?;
                    assert_eq!(
                        (phase, deadline, source.as_str()),
                        (expected, base + GUARD_MAX_WAIT, SOURCE)
                    );
                    assert_eq!(guard.gate.0.load(Ordering::Acquire), State::Active as u8);
                    let nested = guard.present_until_with(
                        input(SourcePresentationMode::SourceOnly).expect("bounded input"),
                        base + GUARD_MAX_WAIT,
                        || base,
                        |_, _| panic!("busy must not wait"),
                    );
                    assert_eq!(nested.err(), Some(GuardFallback::Busy));
                    release
                        .send(())
                        .map_err(|_| RecvTimeoutError::Disconnected)?;
                }
                receiver.recv_timeout(WAIT)
            },
        )
        .map_err(|_| "combined job failed")?;
    assert_eq!(
        result.forecast().expect("forecast candidate").edits().len(),
        2
    );
    assert_eq!(
        result.description().disposition(),
        SourceDescriptionGuardDisposition::CompleteSyntax
    );
    finished.recv_timeout(WAIT)?;
    assert_eq!(guard.gate.0.load(Ordering::Acquire), State::Ready as u8);
    Ok(())
}

#[test]
fn timeout_during_fallback_keeps_admission_and_discards_first_candidate() -> TestResult {
    let base = Instant::now();
    let (phase_tx, phases) = sync_channel(1);
    let (release, release_rx) = sync_channel(1);
    let (finished_tx, finished) = sync_channel(1);
    let guard = SourceAnalysisGuard::with_job_worker(
        PausedWorker {
            analyzer: Analyzer::new()?,
            phases: phase_tx,
            release: release_rx,
        },
        move || {
            let _ = finished_tx.try_send(());
        },
        move || base,
    );
    let result = guard.present_until_with(
        input(SourcePresentationMode::ConditionalForecast)?,
        base + GUARD_MAX_WAIT,
        || base,
        |_, _| {
            assert_eq!(
                phases
                    .recv_timeout(WAIT)
                    .map_err(|_| RecvTimeoutError::Disconnected)?
                    .0,
                Phase::Forecast
            );
            release
                .send(())
                .map_err(|_| RecvTimeoutError::Disconnected)?;
            assert_eq!(
                phases
                    .recv_timeout(WAIT)
                    .map_err(|_| RecvTimeoutError::Disconnected)?
                    .0,
                Phase::Description
            );
            Err(RecvTimeoutError::Timeout)
        },
    );
    assert_eq!(result.err(), Some(GuardFallback::Deadline));
    let next = guard.present_until_with(
        input(SourcePresentationMode::SourceOnly)?,
        base + GUARD_MAX_WAIT,
        || base,
        |_, _| panic!("stale work still owns admission"),
    );
    assert_eq!(next.err(), Some(GuardFallback::Busy));
    release.send(())?;
    finished.recv_timeout(WAIT)?;
    assert_eq!(guard.gate.0.load(Ordering::Acquire), State::Ready as u8);
    Ok(())
}

#[test]
fn source_only_combined_job_never_runs_forecast() -> TestResult {
    let base = Instant::now();
    let (phase_tx, phases) = sync_channel(1);
    let (release, release_rx) = sync_channel(1);
    let guard = SourceAnalysisGuard::with_job_worker(
        PausedWorker {
            analyzer: Analyzer::new()?,
            phases: phase_tx,
            release: release_rx,
        },
        || {},
        move || base,
    );
    let result = guard
        .present_until_with(
            input(SourcePresentationMode::SourceOnly)?,
            base + GUARD_MAX_WAIT,
            || base,
            |receiver, _| {
                assert_eq!(
                    phases
                        .recv_timeout(WAIT)
                        .map_err(|_| RecvTimeoutError::Disconnected)?
                        .0,
                    Phase::Description
                );
                release
                    .send(())
                    .map_err(|_| RecvTimeoutError::Disconnected)?;
                receiver.recv_timeout(WAIT)
            },
        )
        .map_err(|_| "source-only job failed")?;
    assert!(result.forecast().is_none());
    assert_eq!(
        result.description().disposition(),
        SourceDescriptionGuardDisposition::CompleteSyntax
    );
    Ok(())
}

#[test]
fn expired_combined_job_never_acquires_or_enqueues() -> TestResult {
    let base = Instant::now();
    let (jobs, queued) = sync_channel(1);
    let guard = SourceAnalysisGuard {
        jobs,
        gate: Arc::new(Gate(AtomicU8::new(State::Ready as u8))),
    };
    let result = guard.present_until_with(
        input(SourcePresentationMode::ConditionalForecast)?,
        base,
        || base,
        |_, _| panic!("expired work cannot wait"),
    );
    assert_eq!(result.err(), Some(GuardFallback::Deadline));
    assert!(matches!(queued.try_recv(), Err(TryRecvError::Empty)));
    assert_eq!(guard.gate.0.load(Ordering::Acquire), State::Ready as u8);
    Ok(())
}
