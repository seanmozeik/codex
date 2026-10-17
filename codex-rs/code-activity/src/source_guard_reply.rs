//! Reply ownership retains admission until disposal and disables damaged workers.

use super::Gate;
use crate::GuardFallback;
use std::sync::Arc;
use std::sync::mpsc::SyncSender;
use std::time::Instant;

// Disable before dropping a sender during unwinding. An awakened caller must
// never admit new work to an analyzer damaged by the unwinding job.
pub(super) struct PendingReply<T> {
    pub(super) reply: Option<SyncSender<Completed<Result<T, GuardFallback>>>>,
    pub(super) gate: Arc<Gate>,
}

impl<T> PendingReply<T> {
    pub(super) fn finish(mut self, activity: Result<T, GuardFallback>) -> bool {
        let failed = matches!(activity, Err(GuardFallback::WorkerUnavailable));
        if failed {
            self.gate.disable();
        }
        if let Some(sender) = self.reply.take() {
            // A departed caller's complete result is disposed before releasing admission.
            let _ = sender.try_send(Completed {
                value: activity,
                completion: Completion(Arc::clone(&self.gate)),
            });
            #[cfg(test)]
            tests::after_send();
            drop(sender);
        }
        failed
    }
}

#[cfg(test)]
#[path = "source_guard_reply_tests.rs"]
mod tests;

impl<T> Drop for PendingReply<T> {
    fn drop(&mut self) {
        if self.reply.is_some() {
            self.gate.disable();
        }
    }
}

pub(super) struct DisableOnExit(pub(super) Arc<Gate>);
impl Drop for DisableOnExit {
    fn drop(&mut self) {
        self.0.disable();
    }
}

// Field order disposes an unaccepted payload before releasing its admission.
// Completion still runs during unwinding if payload disposal panics.
pub(in crate::source_guard) struct Completed<T> {
    value: T,
    completion: Completion,
}

impl<T> Completed<T> {
    fn into_inner(self) -> T {
        let Self { value, completion } = self;
        drop(completion);
        value
    }
}

impl<T> Completed<Result<T, GuardFallback>> {
    pub(in crate::source_guard) fn at_deadline(
        self,
        deadline: Instant,
        now: Instant,
    ) -> Result<T, GuardFallback> {
        if now >= deadline {
            return Err(GuardFallback::Deadline);
        }
        self.into_inner()
    }
}

struct Completion(Arc<Gate>);

impl Drop for Completion {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.0.disable();
        } else {
            self.0.finish();
        }
    }
}

#[cfg(test)]
#[path = "source_guard_completion_tests.rs"]
mod completion_tests;
