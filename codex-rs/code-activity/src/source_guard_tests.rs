//! Barrier-driven ownership and deadline tests; snippets are never executed.

use super::GUARD_MAX_CALL_ID_BYTES;
use super::GUARD_MAX_CWD_BYTES;
use super::GUARD_MAX_SOURCE_BYTES;
use super::GUARD_MAX_WAIT;
use super::Gate;
use super::SourceAnalysisGuard;
use super::State;
use crate::AnalysisLimits;
use crate::Analyzer;
use crate::CompleteScript;
use crate::EnvironmentAssumptions;
use crate::FileSnapshot;
use crate::GuardDisposition;
use crate::GuardFallback;
use crate::GuardedSourceActivity;
use crate::Language;
use crate::Request;
use pretty_assertions::assert_eq;
use std::error::Error;
use std::sync::Arc;
use std::sync::Barrier;
use std::sync::atomic::AtomicU8;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::mpsc::sync_channel;
use std::time::Duration;
use std::time::Instant;

type TestResult = Result<(), Box<dyn Error>>;
const TEST_WAIT: Duration = Duration::from_secs(5);

fn script(source: &str) -> Result<CompleteScript, crate::ActivityError> {
    CompleteScript::from_request(
        Request::new(
            "repeated-public-id".into(),
            Language::Python,
            source.into(),
            None,
        ),
        EnvironmentAssumptions::FreshProcess,
        AnalysisLimits::new(crate::MAX_SOURCE_BYTES, TEST_WAIT),
    )
}

fn worker(
    work: impl FnMut(CompleteScript, Instant) -> super::Response + Send + 'static,
    finished: impl FnMut() + Send + 'static,
) -> SourceAnalysisGuard {
    let now = Instant::now();
    SourceAnalysisGuard::with_worker_clock(work, finished, move || now)
}

fn analyze(guard: &SourceAnalysisGuard, script: CompleteScript) -> GuardedSourceActivity {
    let started = Instant::now();
    guard.analyze_with(
        script,
        GUARD_MAX_WAIT,
        || started,
        |receiver, _| receiver.recv_timeout(TEST_WAIT),
    )
}

fn fallback(activity: &GuardedSourceActivity, reason: GuardFallback) {
    assert_eq!(activity.disposition(), GuardDisposition::Fallback(reason));
    assert!(activity.eligible_activity().is_none());
    assert!(activity.ui_rows().is_empty());
    assert!(activity.diffs().is_empty());
}

#[test]
fn timed_out_job_holds_admission_and_cannot_attach_to_repeated_id() -> TestResult {
    let started = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let worker_started = Arc::clone(&started);
    let worker_release = Arc::clone(&release);
    let (retired, completed) = sync_channel(2);
    let mut analyzer = Analyzer::new()?;
    let mut first = true;
    let guard = worker(
        move |script, _| {
            if first {
                first = false;
                worker_started.wait();
                worker_release.wait();
            }
            Ok(analyzer.analyze_complete(script))
        },
        move || {
            let _ = retired.try_send(());
        },
    );
    let clock = Instant::now();
    let late = guard.analyze_with(
        script("open('stale').read()")?,
        GUARD_MAX_WAIT,
        || clock,
        |_, _| {
            started.wait();
            Err(RecvTimeoutError::Timeout)
        },
    );
    fallback(&late, GuardFallback::Deadline);
    fallback(
        &guard.analyze(script("open('fresh').read()")?, GUARD_MAX_WAIT),
        GuardFallback::Busy,
    );
    release.wait();
    completed.recv_timeout(TEST_WAIT)?;
    let fresh = analyze(&guard, script("open('fresh').read()")?);
    assert_eq!(fresh.disposition(), GuardDisposition::Eligible);
    let report = serde_json::to_string(
        fresh
            .eligible_activity()
            .ok_or("missing eligible activity")?
            .report(),
    )?;
    assert!(report.contains("fresh"));
    assert!(!report.contains("stale"));
    completed.recv_timeout(TEST_WAIT)?;
    Ok(())
}

#[test]
fn receipt_after_deadline_discards_completed_result() -> TestResult {
    let mut analyzer = Analyzer::new()?;
    let guard = worker(
        move |script, _| Ok(analyzer.analyze_complete(script)),
        || {},
    );
    let start = Instant::now();
    let mut reads = 0;
    let result = guard.analyze_with(
        script("open('a').read()")?,
        GUARD_MAX_WAIT,
        || {
            reads += 1;
            if reads < 3 {
                start
            } else {
                start + GUARD_MAX_WAIT
            }
        },
        |receiver, _| receiver.recv_timeout(TEST_WAIT),
    );
    fallback(&result, GuardFallback::Deadline);
    Ok(())
}

#[test]
fn requested_wait_is_clamped_and_zero_wait_never_admits() -> TestResult {
    let mut analyzer = Analyzer::new()?;
    let guard = worker(
        move |script, _| Ok(analyzer.analyze_complete(script)),
        || {},
    );
    fallback(
        &guard.analyze(script("open('a').read()")?, Duration::ZERO),
        GuardFallback::Deadline,
    );
    let start = Instant::now();
    let result = guard.analyze_with(
        script("open('a').read()")?,
        Duration::MAX,
        || start,
        |receiver, remaining| {
            assert_eq!(remaining, GUARD_MAX_WAIT);
            receiver.recv_timeout(TEST_WAIT)
        },
    );
    assert_eq!(result.disposition(), GuardDisposition::Eligible);
    Ok(())
}

#[test]
fn unsupported_suffix_discards_recognized_prefix() -> TestResult {
    let mut analyzer = Analyzer::new()?;
    let guard = worker(
        move |script, _| Ok(analyzer.analyze_complete(script)),
        || {},
    );
    let result = analyze(&guard, script("open('a').read(); unknown()")?);
    fallback(
        &result,
        GuardFallback::Source(crate::FallbackReason::UnresolvedSource),
    );
    Ok(())
}

#[test]
fn field_caps_snapshots_and_unknown_environment_never_invoke_worker() -> TestResult {
    let calls = Arc::new(AtomicUsize::new(0));
    let worker_calls = Arc::clone(&calls);
    let guard = worker(
        move |_, _| {
            worker_calls.fetch_add(1, Ordering::SeqCst);
            Err(GuardFallback::WorkerUnavailable)
        },
        || {},
    );
    let mut unknown = script("open('a').read()")?;
    unknown.environment = EnvironmentAssumptions::PersistentOrUnknown;
    fallback(
        &guard.analyze(unknown, GUARD_MAX_WAIT),
        GuardFallback::EnvironmentUnknown,
    );
    let oversized = script(&" ".repeat(GUARD_MAX_SOURCE_BYTES + 1))?;
    fallback(
        &guard.analyze(oversized, GUARD_MAX_WAIT),
        GuardFallback::RequestTooLarge,
    );
    let mut identity = script("")?;
    identity.request.call_id = "é".repeat(GUARD_MAX_CALL_ID_BYTES / 2 + 1).into();
    fallback(
        &guard.analyze(identity, GUARD_MAX_WAIT),
        GuardFallback::RequestTooLarge,
    );
    let mut cwd = script("")?;
    cwd.request.cwd = Some("x".repeat(GUARD_MAX_CWD_BYTES + 1));
    fallback(
        &guard.analyze(cwd, GUARD_MAX_WAIT),
        GuardFallback::RequestTooLarge,
    );
    let mut snapshots = script("")?;
    snapshots.request.snapshots.push(FileSnapshot {
        path: String::new(),
        before: None,
        after: None,
    });
    fallback(
        &guard.analyze(snapshots, GUARD_MAX_WAIT),
        GuardFallback::SnapshotsUnsupported,
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn exact_logical_byte_caps_are_accepted() -> TestResult {
    let mut analyzer = Analyzer::new()?;
    let guard = worker(
        move |script, _| Ok(analyzer.analyze_complete(script)),
        || {},
    );
    let mut input = script(&format!("#{}", " ".repeat(GUARD_MAX_SOURCE_BYTES - 1)))?;
    input.request.call_id = "x".repeat(GUARD_MAX_CALL_ID_BYTES).into();
    input.request.cwd = Some("x".repeat(GUARD_MAX_CWD_BYTES));
    assert_eq!(
        analyze(&guard, input).disposition(),
        GuardDisposition::Eligible
    );
    Ok(())
}

#[test]
fn worker_failure_disables_service_without_replacement() -> TestResult {
    let (retired, completed) = sync_channel(1);
    let calls = Arc::new(AtomicUsize::new(0));
    let worker_calls = Arc::clone(&calls);
    let guard = worker(
        move |_, _| {
            worker_calls.fetch_add(1, Ordering::SeqCst);
            Err(GuardFallback::WorkerUnavailable)
        },
        move || {
            let _ = retired.try_send(());
        },
    );
    fallback(
        &analyze(&guard, script("")?),
        GuardFallback::WorkerUnavailable,
    );
    completed.recv_timeout(TEST_WAIT)?;
    fallback(
        &analyze(&guard, script("")?),
        GuardFallback::WorkerUnavailable,
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    Ok(())
}

#[test]
fn unwinding_disables_service_before_response_disconnect() -> TestResult {
    let guard = worker(|_, _| std::panic::resume_unwind(Box::new(())), || {});
    fallback(
        &analyze(&guard, script("")?),
        GuardFallback::WorkerUnavailable,
    );
    fallback(
        &analyze(&guard, script("")?),
        GuardFallback::WorkerUnavailable,
    );
    Ok(())
}

#[test]
fn disconnected_job_receiver_permanently_disables_admission() -> TestResult {
    let (jobs, receiver) = sync_channel(1);
    drop(receiver);
    let guard = SourceAnalysisGuard {
        jobs,
        gate: Arc::new(Gate(AtomicU8::new(State::Ready as u8))),
    };
    fallback(
        &analyze(&guard, script("")?),
        GuardFallback::WorkerUnavailable,
    );
    fallback(
        &analyze(&guard, script("")?),
        GuardFallback::WorkerUnavailable,
    );
    Ok(())
}

#[test]
fn public_shared_handle_is_a_singleton() {
    assert!(std::ptr::eq(
        SourceAnalysisGuard::shared(),
        SourceAnalysisGuard::shared()
    ));
}
