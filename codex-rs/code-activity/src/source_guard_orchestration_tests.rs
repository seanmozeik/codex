//! Barrier-driven cross-kind ownership tests; authored source is never executed.

use super::GUARD_MAX_WAIT;
use super::SourceAnalysisGuard;
use super::initialize_and_budget;
use crate::AnalysisLimits;
use crate::Analyzer;
use crate::CompleteOrchestration;
use crate::CompleteScript;
use crate::EnvironmentAssumptions;
use crate::GuardDisposition;
use crate::GuardFallback;
use crate::GuardedOrchestrationActivity;
use crate::Language;
use crate::OrchestratorBindings;
use crate::Request;
use pretty_assertions::assert_eq;
use std::cell::Cell;
use std::error::Error;
use std::sync::Arc;
use std::sync::Barrier;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::mpsc::sync_channel;
use std::time::Duration;
use std::time::Instant;

type TestResult = Result<(), Box<dyn Error>>;
const WAIT: Duration = Duration::from_secs(5);
const FRESH: &str = r#"await tools.exec_command({cmd: "python -c \"open('fresh').read()\""});"#;
const STALE: &str = r#"await tools.exec_command({cmd: "python -c \"open('stale').read()\""});"#;

fn envelope(source: &str) -> Result<CompleteOrchestration, crate::OrchestrationInputError> {
    CompleteOrchestration::from_source(
        "same-public-id".into(),
        source,
        OrchestratorBindings::TrustedFreshHostBindings,
        EnvironmentAssumptions::FreshProcess,
        AnalysisLimits::new(super::GUARD_MAX_SOURCE_BYTES, WAIT),
    )
}

fn script() -> Result<CompleteScript, crate::ActivityError> {
    CompleteScript::from_request(
        Request::new(
            "same-public-id".into(),
            Language::Python,
            "open('a').read()".into(),
            None,
        ),
        EnvironmentAssumptions::FreshProcess,
        AnalysisLimits::new(1024, WAIT),
    )
}

fn fallback(result: &GuardedOrchestrationActivity, reason: GuardFallback) {
    assert_eq!(result.disposition(), GuardDisposition::Fallback(reason));
    assert!(result.eligible_activity().is_none());
    assert!(result.groups().is_empty());
}

#[test]
fn timed_out_envelope_keeps_script_busy_and_cannot_attach_to_reused_identity() -> TestResult {
    let base = Instant::now();
    let started = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let worker_started = Arc::clone(&started);
    let worker_release = Arc::clone(&release);
    let (retired, completed) = sync_channel(2);
    let mut analyzer = Analyzer::new()?;
    let mut first = true;
    let guard = SourceAnalysisGuard::with_workers_clock(
        |_, _| Err(GuardFallback::WorkerUnavailable),
        move |input, _| {
            if first {
                first = false;
                worker_started.wait();
                worker_release.wait();
            }
            Ok(analyzer.analyze_orchestration(input))
        },
        move || {
            let _ = retired.try_send(());
        },
        move || base,
    );
    let result = guard.analyze_orchestration_with(
        envelope(STALE)?,
        GUARD_MAX_WAIT,
        || base,
        |_, _| {
            started.wait();
            Err(RecvTimeoutError::Timeout)
        },
    );
    fallback(&result, GuardFallback::Deadline);
    assert_eq!(
        guard.analyze(script()?, GUARD_MAX_WAIT).disposition(),
        GuardDisposition::Fallback(GuardFallback::Busy)
    );
    fallback(
        &guard.analyze_orchestration(envelope(FRESH)?, GUARD_MAX_WAIT),
        GuardFallback::Busy,
    );
    release.wait();
    completed.recv_timeout(WAIT)?;
    let fresh = guard.analyze_orchestration_with(
        envelope(FRESH)?,
        GUARD_MAX_WAIT,
        || base,
        |receiver, _| receiver.recv_timeout(WAIT),
    );
    completed.recv_timeout(WAIT)?;
    assert_eq!(fresh.disposition(), GuardDisposition::Eligible);
    let activity = fresh
        .eligible_activity()
        .ok_or("missing complete activity")?;
    assert_eq!(activity.source(), FRESH);
    assert!(!serde_json::to_string(activity)?.contains("stale"));
    Ok(())
}

#[test]
fn timed_out_script_keeps_envelope_busy_until_worker_cleanup() -> TestResult {
    let base = Instant::now();
    let started = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let worker_started = Arc::clone(&started);
    let worker_release = Arc::clone(&release);
    let (retired, completed) = sync_channel(1);
    let guard = SourceAnalysisGuard::with_workers_clock(
        move |_, _| {
            worker_started.wait();
            worker_release.wait();
            Err(GuardFallback::Deadline)
        },
        |_, _| Err(GuardFallback::WorkerUnavailable),
        move || {
            let _ = retired.try_send(());
        },
        move || base,
    );
    let result = guard.analyze_with(
        script()?,
        GUARD_MAX_WAIT,
        || base,
        |_, _| {
            started.wait();
            Err(RecvTimeoutError::Timeout)
        },
    );
    assert_eq!(
        result.disposition(),
        GuardDisposition::Fallback(GuardFallback::Deadline)
    );
    fallback(
        &guard.analyze_orchestration(envelope(FRESH)?, GUARD_MAX_WAIT),
        GuardFallback::Busy,
    );
    release.wait();
    completed.recv_timeout(WAIT)?;
    Ok(())
}

#[test]
fn envelope_unknown_assumptions_and_zero_wait_never_admit() -> TestResult {
    let calls = Arc::new(AtomicUsize::new(0));
    let worker_calls = Arc::clone(&calls);
    let base = Instant::now();
    let guard = SourceAnalysisGuard::with_workers_clock(
        |_, _| Err(GuardFallback::WorkerUnavailable),
        move |_, _| {
            worker_calls.fetch_add(1, Ordering::SeqCst);
            Err(GuardFallback::WorkerUnavailable)
        },
        || {},
        move || base,
    );
    for (bindings, environment) in [
        (
            OrchestratorBindings::Unknown,
            EnvironmentAssumptions::FreshProcess,
        ),
        (
            OrchestratorBindings::TrustedFreshHostBindings,
            EnvironmentAssumptions::PersistentOrUnknown,
        ),
    ] {
        let input = CompleteOrchestration::from_source(
            "id".into(),
            FRESH,
            bindings,
            environment,
            AnalysisLimits::new(1024, WAIT),
        )?;
        fallback(
            &guard.analyze_orchestration(input, GUARD_MAX_WAIT),
            GuardFallback::EnvironmentUnknown,
        );
    }
    fallback(
        &guard.analyze_orchestration(envelope(FRESH)?, Duration::ZERO),
        GuardFallback::Deadline,
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn envelope_receipt_at_deadline_discards_all_groups() -> TestResult {
    let base = Instant::now();
    let mut analyzer = Analyzer::new()?;
    let guard = SourceAnalysisGuard::with_workers_clock(
        |_, _| Err(GuardFallback::WorkerUnavailable),
        move |input, _| Ok(analyzer.analyze_orchestration(input)),
        || {},
        move || base,
    );
    let mut clock = 0;
    let result = guard.analyze_orchestration_with(
        envelope(FRESH)?,
        GUARD_MAX_WAIT,
        || {
            clock += 1;
            if clock < 3 {
                base
            } else {
                base + GUARD_MAX_WAIT
            }
        },
        |receiver, _| receiver.recv_timeout(WAIT),
    );
    fallback(&result, GuardFallback::Deadline);
    Ok(())
}

#[test]
fn envelope_worker_unwind_permanently_disables_both_request_kinds() -> TestResult {
    let base = Instant::now();
    let guard = SourceAnalysisGuard::with_workers_clock(
        |_, _| Err(GuardFallback::Deadline),
        |_, _| std::panic::resume_unwind(Box::new(())),
        || {},
        move || base,
    );
    let result = guard.analyze_orchestration_with(
        envelope(FRESH)?,
        GUARD_MAX_WAIT,
        || base,
        |receiver, _| receiver.recv_timeout(WAIT),
    );
    fallback(&result, GuardFallback::WorkerUnavailable);
    assert_eq!(
        guard.analyze(script()?, GUARD_MAX_WAIT).disposition(),
        GuardDisposition::Fallback(GuardFallback::WorkerUnavailable)
    );
    fallback(
        &guard.analyze_orchestration(envelope(FRESH)?, GUARD_MAX_WAIT),
        GuardFallback::WorkerUnavailable,
    );
    Ok(())
}

#[test]
fn envelope_queue_and_setup_time_reduce_one_original_budget() -> TestResult {
    let base = Instant::now();
    let mut analyzer = None;
    let mut calls = 0;
    let input = initialize_and_budget(
        envelope(FRESH)?,
        base + GUARD_MAX_WAIT,
        &mut analyzer,
        || Some(()),
        || {
            calls += 1;
            base + Duration::from_millis(if calls == 1 { 10 } else { 70 })
        },
    )
    .map_err(|_| "unexpected fallback")?;
    assert_eq!(input.limits().elapsed(), Duration::from_millis(30));
    assert_eq!(input.source(), FRESH);
    let initialized = Cell::new(false);
    let mut fresh = None;
    let expired = initialize_and_budget(
        envelope(FRESH)?,
        base,
        &mut fresh,
        || {
            initialized.set(true);
            Some(())
        },
        || base,
    );
    assert_eq!(expired.err(), Some(GuardFallback::Deadline));
    assert!(!initialized.get());
    Ok(())
}

#[test]
fn unsupported_envelope_suffix_discards_eligible_child_groups() -> TestResult {
    let base = Instant::now();
    let mut analyzer = Analyzer::new()?;
    let guard = SourceAnalysisGuard::with_workers_clock(
        |_, _| Err(GuardFallback::WorkerUnavailable),
        move |input, _| Ok(analyzer.analyze_orchestration(input)),
        || {},
        move || base,
    );
    let input = envelope(&format!("{FRESH}\nunknown();"))?;
    let result = guard.analyze_orchestration_with(
        input,
        GUARD_MAX_WAIT,
        || base,
        |receiver, _| receiver.recv_timeout(WAIT),
    );
    fallback(
        &result,
        GuardFallback::Orchestration(crate::OrchestrationFallback::UnsupportedSource),
    );
    let value = serde_json::to_value(&result)?;
    assert_eq!(value["type"], "fallback");
    assert!(value.get("groups").is_none());
    Ok(())
}
