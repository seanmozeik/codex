//! Worker scheduling/setup budget tests use explicit monotonic clock values.

use super::GUARD_MAX_WAIT;
use super::SourceAnalysisGuard;
use super::initialize_and_budget;
use crate::AnalysisLimits;
use crate::Analyzer;
use crate::CompleteScript;
use crate::EnvironmentAssumptions;
use crate::GuardDisposition;
use crate::GuardFallback;
use crate::Language;
use crate::Request;
use pretty_assertions::assert_eq;
use std::cell::Cell;
use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::mpsc::sync_channel;
use std::time::Duration;
use std::time::Instant;

type TestResult = Result<(), Box<dyn Error>>;

fn script(elapsed: Duration) -> Result<CompleteScript, crate::ActivityError> {
    CompleteScript::from_request(
        Request::new(
            "deadline-test".into(),
            Language::Python,
            "open('a').read()".into(),
            None,
        ),
        EnvironmentAssumptions::FreshProcess,
        AnalysisLimits::new(1024, elapsed),
    )
}

#[test]
fn expired_scheduled_job_skips_work_and_releases_admission_for_next_job() -> TestResult {
    let base = Instant::now();
    let next = base + Duration::from_secs(1);
    let calls = Arc::new(AtomicUsize::new(0));
    let worker_calls = Arc::clone(&calls);
    let (retired, completed) = sync_channel(2);
    let mut analyzer = Analyzer::new()?;
    let mut clock_calls = 0;
    let guard = SourceAnalysisGuard::with_worker_clock(
        move |script, _| {
            worker_calls.fetch_add(1, Ordering::SeqCst);
            Ok(analyzer.analyze_complete(script))
        },
        move || {
            let _ = retired.try_send(());
        },
        move || {
            clock_calls += 1;
            if clock_calls == 1 {
                base + GUARD_MAX_WAIT
            } else {
                next
            }
        },
    );
    let expired = guard.analyze_with(
        script(Duration::from_secs(5))?,
        GUARD_MAX_WAIT,
        || base,
        |receiver, _| receiver.recv_timeout(Duration::from_secs(5)),
    );
    completed.recv_timeout(Duration::from_secs(5))?;
    assert_eq!(
        expired.disposition(),
        GuardDisposition::Fallback(GuardFallback::Deadline)
    );
    assert!(expired.ui_rows().is_empty());
    assert!(expired.diffs().is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let fresh = guard.analyze_with(
        script(Duration::from_secs(5))?,
        GUARD_MAX_WAIT,
        || next,
        |receiver, _| receiver.recv_timeout(Duration::from_secs(5)),
    );
    completed.recv_timeout(Duration::from_secs(5))?;
    assert_eq!(fresh.disposition(), GuardDisposition::Eligible);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    Ok(())
}

#[test]
fn expired_job_does_not_initialize_analyzer() -> TestResult {
    let now = Instant::now();
    let calls = Cell::new(0);
    let mut analyzer = None;
    let result = initialize_and_budget(
        script(GUARD_MAX_WAIT)?,
        now,
        &mut analyzer,
        || {
            calls.set(calls.get() + 1);
            Some(())
        },
        || now,
    );
    assert_eq!(result.err(), Some(GuardFallback::Deadline));
    assert_eq!(calls.get(), 0);
    assert!(analyzer.is_none());
    Ok(())
}

#[test]
fn setup_consuming_deadline_skips_analysis_without_disabling_analyzer() -> TestResult {
    let now = Instant::now();
    let deadline = now + GUARD_MAX_WAIT;
    let mut calls = 0;
    let mut analyzer = None;
    let result = initialize_and_budget(
        script(Duration::from_secs(5))?,
        deadline,
        &mut analyzer,
        || Some(()),
        || {
            calls += 1;
            if calls == 1 { now } else { deadline }
        },
    );
    assert_eq!(result.err(), Some(GuardFallback::Deadline));
    assert_eq!(analyzer, Some(()));
    let next = initialize_and_budget(
        script(GUARD_MAX_WAIT)?,
        deadline + GUARD_MAX_WAIT,
        &mut analyzer,
        || None,
        || deadline,
    )
    .map_err(|_| "unexpected fallback")?;
    assert_eq!(next.limits.elapsed(), GUARD_MAX_WAIT);
    Ok(())
}

#[test]
fn setup_and_queue_time_reduce_budget_without_extending_original_limit() -> TestResult {
    let base = Instant::now();
    let deadline = base + GUARD_MAX_WAIT;
    let mut analyzer = None;
    for (original, expected) in [
        (Duration::from_millis(20), Duration::from_millis(20)),
        (Duration::from_secs(5), Duration::from_millis(30)),
    ] {
        let mut clock_calls = 0;
        let input = initialize_and_budget(
            script(original)?,
            deadline,
            &mut analyzer,
            || Some(()),
            || {
                clock_calls += 1;
                base + Duration::from_millis(if clock_calls == 1 { 10 } else { 70 })
            },
        )
        .map_err(|_| "unexpected fallback")?;
        assert_eq!(input.limits.elapsed(), expected);
        assert_eq!(input.limits.source_bytes(), 1024);
    }
    Ok(())
}
