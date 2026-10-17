//! Test-only injectable workers; no submitted source is executed.

use super::Completed;
use super::DescriptionResponse;
use super::EnvelopeResponse;
use super::Response;
use super::SourceAnalysisGuard;
use super::Worker;
use super::reply::PendingReply;
use crate::CompleteOrchestration;
use crate::CompleteScript;
use crate::CompleteSourceDescriptionInput;
use crate::GuardFallback;
use std::sync::Arc;
use std::sync::mpsc::SyncSender;
use std::time::Instant;

impl SourceAnalysisGuard {
    pub(in crate::source_guard) fn finish_test_reply<T>(
        &self,
        reply: SyncSender<Completed<Result<T, GuardFallback>>>,
        activity: Result<T, GuardFallback>,
    ) {
        PendingReply {
            reply: Some(reply),
            gate: Arc::clone(&self.gate),
        }
        .finish(activity);
    }

    #[cfg(test)]
    pub(in crate::source_guard) fn with_worker_clock(
        work: impl FnMut(CompleteScript, Instant) -> Response + Send + 'static,
        finished: impl FnMut() + Send + 'static,
        now: impl FnMut() -> Instant + Send + 'static,
    ) -> Self {
        Self::with_workers_clock(
            work,
            |_, _| Err(GuardFallback::WorkerUnavailable),
            finished,
            now,
        )
    }

    #[cfg(test)]
    pub(in crate::source_guard) fn with_workers_clock(
        script: impl FnMut(CompleteScript, Instant) -> Response + Send + 'static,
        orchestration: impl FnMut(CompleteOrchestration, Instant) -> EnvelopeResponse + Send + 'static,
        finished: impl FnMut() + Send + 'static,
        now: impl FnMut() -> Instant + Send + 'static,
    ) -> Self {
        Self::with_job_worker(
            TestWorker {
                script,
                orchestration,
            },
            finished,
            now,
        )
    }
}

#[cfg(test)]
struct TestWorker<S, O> {
    script: S,
    orchestration: O,
}

#[cfg(test)]
impl<S, O> Worker for TestWorker<S, O>
where
    S: FnMut(CompleteScript, Instant) -> Response + Send + 'static,
    O: FnMut(CompleteOrchestration, Instant) -> EnvelopeResponse + Send + 'static,
{
    fn description(
        &mut self,
        _input: CompleteSourceDescriptionInput,
        _deadline: Instant,
    ) -> DescriptionResponse {
        Err(GuardFallback::WorkerUnavailable)
    }
    fn script(&mut self, input: CompleteScript, deadline: Instant) -> Response {
        (self.script)(input, deadline)
    }
    fn orchestration(
        &mut self,
        input: CompleteOrchestration,
        deadline: Instant,
    ) -> EnvelopeResponse {
        (self.orchestration)(input, deadline)
    }
}
