//! Ownership controls for queued, expired, and unwinding transport replies.

use super::super::State;
use super::Completed;
use super::Gate;
use super::PendingReply;
use crate::GuardFallback;
use pretty_assertions::assert_eq;
use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::AtomicU8;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::SyncSender;
use std::sync::mpsc::sync_channel;
use std::time::Duration;
use std::time::Instant;

type TestResult = Result<(), Box<dyn Error>>;
type ReplyReceiver<T> = Receiver<Completed<Result<T, GuardFallback>>>;
const WAIT: Duration = Duration::from_secs(5);

fn queued<T>(value: Result<T, GuardFallback>) -> (Arc<Gate>, ReplyReceiver<T>) {
    let gate = Arc::new(Gate(AtomicU8::new(State::Active as u8)));
    let (sender, receiver) = sync_channel(1);
    PendingReply {
        reply: Some(sender),
        gate: Arc::clone(&gate),
    }
    .finish(value);
    (gate, receiver)
}

#[derive(Clone, Copy)]
enum Disposal {
    Queued,
    Expired,
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

fn disposal_retains_admission(path: Disposal) -> TestResult {
    let (started, dropping) = sync_channel(1);
    let (release, paused) = sync_channel(1);
    let (gate, receiver) = queued(Ok(BlockingDrop {
        started,
        release: paused,
    }));
    let caller = std::thread::spawn(move || -> Result<(), &'static str> {
        match path {
            Disposal::Queued => drop(receiver),
            Disposal::Expired => {
                let reply = receiver.recv_timeout(WAIT).map_err(|_| "missing reply")?;
                let now = Instant::now();
                let result = reply.at_deadline(now, now);
                if !matches!(result, Err(GuardFallback::Deadline)) {
                    return Err("expired reply escaped its deadline");
                }
            }
        }
        Ok(())
    });
    let started = dropping.recv_timeout(WAIT);
    let during = gate.admit();
    drop(release);
    caller.join().map_err(|_| "caller panicked")??;
    started?;
    assert_eq!(during, Err(GuardFallback::Busy));
    assert_eq!(gate.admit(), Ok(()));
    Ok(())
}

#[test]
fn queued_abandonment_disposes_payload_before_releasing_admission() -> TestResult {
    disposal_retains_admission(Disposal::Queued)
}

#[test]
fn expired_received_reply_disposes_payload_before_releasing_admission() -> TestResult {
    disposal_retains_admission(Disposal::Expired)
}

struct UnwindingDrop;

impl Drop for UnwindingDrop {
    fn drop(&mut self) {
        std::panic::resume_unwind(Box::new(()));
    }
}

fn disposal_unwind_disables(path: Disposal) -> TestResult {
    let (gate, receiver) = queued(Ok(UnwindingDrop));
    // No worker DisableOnExit is present: the receiver's token must disable.
    let caller = std::thread::spawn(move || -> Result<(), &'static str> {
        match path {
            Disposal::Queued => drop(receiver),
            Disposal::Expired => {
                let reply = receiver.recv_timeout(WAIT).map_err(|_| "missing reply")?;
                let now = Instant::now();
                drop(reply.at_deadline(now, now));
            }
        }
        Ok(())
    });
    let joined = caller.join();
    if let Ok(result) = joined {
        result?;
        return Err("payload destructor did not unwind".into());
    }
    assert_eq!(gate.admit(), Err(GuardFallback::WorkerUnavailable));
    Ok(())
}

#[test]
fn queued_abandonment_unwind_disables_without_worker_exit() -> TestResult {
    disposal_unwind_disables(Disposal::Queued)
}

#[test]
fn expired_received_reply_unwind_disables_without_worker_exit() -> TestResult {
    disposal_unwind_disables(Disposal::Expired)
}

struct ReentrantDrop {
    gate: Arc<Gate>,
    observed: SyncSender<Result<(), GuardFallback>>,
}

impl Drop for ReentrantDrop {
    fn drop(&mut self) {
        let _ = self.observed.try_send(self.gate.admit());
    }
}

#[test]
fn payload_disposal_cannot_reenter_admission() -> TestResult {
    let gate = Arc::new(Gate(AtomicU8::new(State::Active as u8)));
    let (observed, receiver) = sync_channel(1);
    let (sender, replies) = sync_channel(1);
    PendingReply {
        reply: Some(sender),
        gate: Arc::clone(&gate),
    }
    .finish(Ok(ReentrantDrop {
        gate: Arc::clone(&gate),
        observed,
    }));
    drop(replies);
    assert_eq!(receiver.recv_timeout(WAIT)?, Err(GuardFallback::Busy));
    assert_eq!(gate.admit(), Ok(()));
    Ok(())
}

#[test]
fn normal_token_release_preserves_disabled_state() -> TestResult {
    let (gate, receiver) = queued(Ok(()));
    gate.disable();
    assert_eq!(receiver.recv_timeout(WAIT)?.into_inner(), Ok(()));
    assert_eq!(gate.admit(), Err(GuardFallback::WorkerUnavailable));
    Ok(())
}

#[test]
fn fatal_reply_disables_before_extraction_and_stays_disabled() -> TestResult {
    let (gate, receiver) = queued::<()>(Err(GuardFallback::WorkerUnavailable));
    let before = gate.admit();
    let result = receiver.recv_timeout(WAIT)?.into_inner();
    assert_eq!(before, Err(GuardFallback::WorkerUnavailable));
    assert_eq!(result, Err(GuardFallback::WorkerUnavailable));
    assert_eq!(gate.admit(), Err(GuardFallback::WorkerUnavailable));
    Ok(())
}

#[test]
fn late_nonfatal_reply_keeps_deadline_precedence() -> TestResult {
    let (gate, receiver) = queued::<()>(Err(GuardFallback::Busy));
    let now = Instant::now();
    let result = receiver.recv_timeout(WAIT)?.at_deadline(now, now);
    assert_eq!(result, Err(GuardFallback::Deadline));
    assert_eq!(gate.admit(), Ok(()));
    Ok(())
}

#[test]
fn timely_nonfatal_reply_releases_admission() -> TestResult {
    let (gate, receiver) = queued::<()>(Err(GuardFallback::Busy));
    let now = Instant::now();
    let result = receiver.recv_timeout(WAIT)?.at_deadline(now + WAIT, now);
    assert_eq!(result, Err(GuardFallback::Busy));
    assert_eq!(gate.admit(), Ok(()));
    Ok(())
}
