//! Deterministic publication and disposal witnesses using the real reply path.

use super::super::State;
use super::DisableOnExit;
use super::Gate;
use super::PendingReply;
use crate::AnalysisLimits;
use crate::Analyzer;
use crate::CompleteScript;
use crate::EnvironmentAssumptions;
use crate::GUARD_MAX_WAIT;
use crate::GuardDisposition;
use crate::GuardFallback;
use crate::Language;
use crate::Request;
use crate::SourceAnalysisGuard;
use pretty_assertions::assert_eq;
use std::cell::Cell;
use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::AtomicU8;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::SyncSender;
use std::sync::mpsc::sync_channel;
use std::time::Duration;
use std::time::Instant;

type TestResult = Result<(), Box<dyn Error>>;
const WAIT: Duration = Duration::from_secs(5);

thread_local! {
    // Only the witness thread installs this; other workers never pause.
    static AFTER_SEND: Cell<Option<Receiver<()>>> = const { Cell::new(None) };
}

pub(super) fn after_send() {
    if let Some(release) = AFTER_SEND.take() {
        let _ = release.recv();
    }
}

#[test]
fn reply_is_published_before_worker_continues() -> TestResult {
    let gate = Arc::new(Gate(AtomicU8::new(State::Active as u8)));
    let worker_gate = Arc::clone(&gate);
    let (sender, receiver) = sync_channel(1);
    let (release, paused) = sync_channel(1);
    let worker = std::thread::spawn(move || {
        AFTER_SEND.set(Some(paused));
        PendingReply {
            reply: Some(sender),
            gate: worker_gate,
        }
        .finish(Ok(()))
    });

    let received = receiver.recv_timeout(WAIT);
    // Always release and join before propagating an error or asserting.
    drop(release);
    let finished = worker.join().map_err(|_| "reply worker panicked")?;
    assert_eq!(received?.into_inner(), Ok(()));
    assert!(!finished);
    Ok(())
}

#[test]
fn returned_guard_result_releases_admission_before_worker_continues() -> TestResult {
    let input = CompleteScript::from_request(
        Request::new(
            "reply-readiness".into(),
            Language::Python,
            "open('a').read()".into(),
            None,
        ),
        EnvironmentAssumptions::FreshProcess,
        AnalysisLimits::new(1024, WAIT),
    )?;
    let mut analyzer = Analyzer::new()?;
    let base = Instant::now();
    let (release, paused) = sync_channel(1);
    let mut checkpoint = Some(paused);
    let (finished, completed) = sync_channel(1);
    let guard = SourceAnalysisGuard::with_worker_clock(
        move |input, _| {
            AFTER_SEND.set(checkpoint.take());
            Ok(analyzer.analyze_complete(input))
        },
        move || {
            let _ = finished.try_send(());
        },
        move || base,
    );

    let activity = guard.analyze_with(
        input,
        GUARD_MAX_WAIT,
        || base,
        |receiver, _| receiver.recv_timeout(WAIT),
    );
    let next_admission = guard.gate.admit();
    drop(release);
    drop(guard);
    // The real worker invokes this only after PendingReply::finish returns.
    completed.recv_timeout(WAIT)?;
    assert_eq!(activity.disposition(), GuardDisposition::Eligible);
    assert_eq!(next_admission, Ok(()));
    Ok(())
}

struct BlockingDrop {
    started: SyncSender<()>,
    release: Receiver<()>,
}

impl Drop for BlockingDrop {
    fn drop(&mut self) {
        let _ = self.started.try_send(());
        let _ = self.release.recv();
    }
}

#[test]
fn disconnected_reply_retains_admission_until_result_disposal() -> TestResult {
    let gate = Arc::new(Gate(AtomicU8::new(State::Active as u8)));
    let worker_gate = Arc::clone(&gate);
    let (sender, receiver) = sync_channel(1);
    drop(receiver);
    let (started, dropping) = sync_channel(1);
    let (release, paused) = sync_channel(1);
    let worker = std::thread::spawn(move || {
        PendingReply {
            reply: Some(sender),
            gate: worker_gate,
        }
        .finish(Ok(BlockingDrop {
            started,
            release: paused,
        }))
    });

    let disposal_started = dropping.recv_timeout(WAIT);
    let during_disposal = gate.admit();
    drop(release);
    let finished = worker.join().map_err(|_| "reply worker panicked")?;
    disposal_started?;
    assert!(!finished);
    assert_eq!(during_disposal, Err(GuardFallback::Busy));
    assert_eq!(gate.admit(), Ok(()));
    Ok(())
}

struct UnwindingDrop;

impl Drop for UnwindingDrop {
    fn drop(&mut self) {
        std::panic::resume_unwind(Box::new(()));
    }
}

#[test]
fn disconnected_reply_disposal_unwind_disables_worker() {
    let gate = Arc::new(Gate(AtomicU8::new(State::Active as u8)));
    let worker_gate = Arc::clone(&gate);
    let (sender, receiver) = sync_channel(1);
    drop(receiver);
    let worker = std::thread::spawn(move || {
        let _disable = DisableOnExit(Arc::clone(&worker_gate));
        PendingReply {
            reply: Some(sender),
            gate: worker_gate,
        }
        .finish(Ok(UnwindingDrop))
    });

    assert!(worker.join().is_err());
    assert_eq!(gate.admit(), Err(GuardFallback::WorkerUnavailable));
}

#[test]
fn late_producer_continuation_cannot_release_new_admission() -> TestResult {
    let input = CompleteScript::from_request(
        Request::new(
            "reply-continuation".into(),
            Language::Python,
            "open('a').read()".into(),
            None,
        ),
        EnvironmentAssumptions::FreshProcess,
        AnalysisLimits::new(1024, WAIT),
    )?;
    let mut analyzer = Analyzer::new()?;
    let base = Instant::now();
    let (release, paused) = sync_channel(1);
    let mut checkpoint = Some(paused);
    let (finished, completed) = sync_channel(1);
    let guard = SourceAnalysisGuard::with_worker_clock(
        move |input, _| {
            AFTER_SEND.set(checkpoint.take());
            Ok(analyzer.analyze_complete(input))
        },
        move || {
            let _ = finished.try_send(());
        },
        move || base,
    );
    let activity = guard.analyze_with(
        input,
        GUARD_MAX_WAIT,
        || base,
        |receiver, _| receiver.recv_timeout(WAIT),
    );
    let next_admission = guard.gate.admit();
    drop(release);
    let finished = completed.recv_timeout(WAIT);
    // Keep the guard alive: worker-exit disable must not mask a late finish.
    let after_continuation = guard.gate.admit();
    guard.gate.finish();
    drop(guard);
    finished?;
    assert_eq!(activity.disposition(), GuardDisposition::Eligible);
    assert_eq!(next_admission, Ok(()));
    assert_eq!(after_continuation, Err(GuardFallback::Busy));
    Ok(())
}

#[test]
fn late_nonfatal_worker_fallback_obeys_guard_deadline() -> TestResult {
    let input = CompleteScript::from_request(
        Request::new(
            "late-fallback".into(),
            Language::Python,
            "pass".into(),
            None,
        ),
        EnvironmentAssumptions::FreshProcess,
        AnalysisLimits::new(1024, WAIT),
    )?;
    let base = Instant::now();
    let (finished, completed) = sync_channel(1);
    let guard = SourceAnalysisGuard::with_worker_clock(
        |_, _| Err(GuardFallback::Busy),
        move || {
            let _ = finished.try_send(());
        },
        move || base,
    );
    let mut calls = 0;
    let activity = guard.analyze_with(
        input,
        GUARD_MAX_WAIT,
        || {
            calls += 1;
            if calls < 3 {
                base
            } else {
                base + GUARD_MAX_WAIT
            }
        },
        |receiver, _| receiver.recv_timeout(WAIT),
    );
    drop(guard);
    completed.recv_timeout(WAIT)?;
    assert_eq!(
        activity.disposition(),
        GuardDisposition::Fallback(GuardFallback::Deadline)
    );
    Ok(())
}

#[test]
fn fatal_reply_is_disabled_while_producer_is_paused() -> TestResult {
    let gate = Arc::new(Gate(AtomicU8::new(State::Active as u8)));
    let worker_gate = Arc::clone(&gate);
    let (sender, receiver) = sync_channel(1);
    let (release, paused) = sync_channel(1);
    let worker = std::thread::spawn(move || {
        AFTER_SEND.set(Some(paused));
        PendingReply::<()> {
            reply: Some(sender),
            gate: worker_gate,
        }
        .finish(Err(GuardFallback::WorkerUnavailable))
    });
    let received = receiver.recv_timeout(WAIT);
    let during_publication = gate.admit();
    drop(release);
    let failed = worker.join().map_err(|_| "reply worker panicked")?;
    let result = received?.into_inner();
    assert!(failed);
    assert_eq!(during_publication, Err(GuardFallback::WorkerUnavailable));
    assert_eq!(result, Err(GuardFallback::WorkerUnavailable));
    assert_eq!(gate.admit(), Err(GuardFallback::WorkerUnavailable));
    Ok(())
}
