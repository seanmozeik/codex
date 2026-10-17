//! Shared admission and one absolute deadline for source analysis and forecasts.

use crate::CompleteOrchestration;
use crate::CompleteScript;
use crate::CompleteSourceDescriptionInput;
use crate::EnvironmentAssumptions;
use crate::GuardFallback;
use crate::GuardedOrchestrationActivity;
use crate::GuardedSourceActivity;
use crate::GuardedSourceDescription;
use crate::OrchestrationDisposition;
use crate::PresentationDisposition;
use crate::SourceDescriptionDisposition;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::mpsc::SyncSender;
use std::sync::mpsc::sync_channel;
use std::time::Duration;
use std::time::Instant;

#[path = "source_guard_forecast.rs"]
mod forecast;
#[path = "source_guard_presentation.rs"]
mod presentation;
#[path = "source_guard_worker.rs"]
mod worker;
pub use presentation::GuardedSourcePresentation;
pub use presentation::SourcePresentationInput;
pub use presentation::SourcePresentationMode;
use worker::AnalyzerWorker;
use worker::Completed;
use worker::Gate;
use worker::Job;
use worker::Response;
#[cfg(test)]
use worker::State;
#[cfg(test)]
use worker::initialize_and_budget;

/// Maximum logical decoded source bytes retained per admitted job.
pub const GUARD_MAX_SOURCE_BYTES: usize = 32 * 1024;
/// Maximum logical call-identity bytes retained per admitted job.
pub const GUARD_MAX_CALL_ID_BYTES: usize = 256;
/// Maximum logical working-directory bytes retained per admitted job.
pub const GUARD_MAX_CWD_BYTES: usize = 4096;
/// Maximum caller-requested response wait, including worker scheduling/setup.
pub const GUARD_MAX_WAIT: Duration = Duration::from_millis(100);

/// Shared, synchronous source enrichment with a single admitted worker job.
///
/// Obtain only through [`Self::shared`]; requests never create workers. Busy,
/// timeout, disconnected, failed, and unsupported requests produce no rows/diffs.
/// A timeout abandons its private response channel but does not release admission
/// until the worker finishes and disposes of the reply. A stuck worker therefore
/// leaves enrichment busy permanently; a failed worker is disabled permanently.
///
/// Bounds cover logical request lengths and job count, not caller allocations,
/// string capacity, total RSS, output allocation, or a CPU-stop guarantee. The
/// deadline bounds response waiting, subject to host scheduling and cleanup; it
/// does not preempt analysis. There is no join-on-drop, restart, or replacement.
/// Unwinding discards the analyzer and disables the service; aborts, OOM, and
/// process-fatal native faults are not isolated. This guard emits no logs and
/// does not replace the host's process-wide panic hook or its disclosure policy.
#[derive(Debug)]
#[non_exhaustive]
pub struct SourceAnalysisGuard {
    jobs: SyncSender<Job>,
    gate: Arc<Gate>,
}

impl SourceAnalysisGuard {
    /// Returns the sole process-wide handle, lazily starting one worker thread.
    /// Analyzer setup is deferred until an admitted job still has deadline budget.
    #[must_use]
    pub fn shared() -> &'static Self {
        static SHARED: OnceLock<SourceAnalysisGuard> = OnceLock::new();
        SHARED.get_or_init(|| Self::with_job_worker(AnalyzerWorker::default(), || {}, Instant::now))
    }

    /// Consumes a complete source-only request and waits up to the capped duration.
    /// Validation precedes admission. Queue and setup time consume the original
    /// deadline; late results are discarded without releasing a running worker.
    /// The host retains its original command and execution behavior on fallback.
    #[must_use]
    pub fn analyze(&self, script: CompleteScript, wait: Duration) -> GuardedSourceActivity {
        self.analyze_with(script, wait, Instant::now, Receiver::recv_timeout)
    }

    /// Analyzes the whole envelope as one job on the same worker as scripts.
    /// Both outer bindings and child environments must be established before
    /// admission. Extraction and all children share one original deadline; no
    /// child is submitted as a new job. Every fallback discards the whole result.
    #[must_use]
    pub fn analyze_orchestration(
        &self,
        input: CompleteOrchestration,
        wait: Duration,
    ) -> GuardedOrchestrationActivity {
        self.analyze_orchestration_with(input, wait, Instant::now, Receiver::recv_timeout)
    }

    /// Describes a complete source on the same single worker used for effects.
    /// No runtime assumptions are accepted. The source-only result cannot supply
    /// file-change rows, observed diffs or permission decisions. Queueing, setup
    /// and every nested source consume one original response deadline.
    #[must_use]
    pub fn describe(
        &self,
        input: CompleteSourceDescriptionInput,
        wait: Duration,
    ) -> GuardedSourceDescription {
        let Some(deadline) = Instant::now().checked_add(wait.min(GUARD_MAX_WAIT)) else {
            return GuardedSourceDescription::fallback(GuardFallback::Deadline);
        };
        self.describe_until(input, deadline)
    }

    /// Describes a complete source without renewing the host's original deadline.
    /// Scheduling before this call and worker setup consume the same budget.
    /// A later supplied deadline is capped to the normal maximum wait.
    #[must_use]
    pub fn describe_until(
        &self,
        input: CompleteSourceDescriptionInput,
        deadline: Instant,
    ) -> GuardedSourceDescription {
        self.describe_until_with(input, deadline, Instant::now, Receiver::recv_timeout)
    }

    fn describe_until_with(
        &self,
        input: CompleteSourceDescriptionInput,
        deadline: Instant,
        mut now: impl FnMut() -> Instant,
        receive: impl FnOnce(
            &Receiver<Completed<worker::DescriptionResponse>>,
            Duration,
        )
            -> Result<Completed<worker::DescriptionResponse>, RecvTimeoutError>,
    ) -> GuardedSourceDescription {
        let current = now();
        let Some(maximum) = current.checked_add(GUARD_MAX_WAIT) else {
            return GuardedSourceDescription::fallback(GuardFallback::Deadline);
        };
        let deadline = deadline.min(maximum);
        if deadline <= current {
            return GuardedSourceDescription::fallback(GuardFallback::Deadline);
        }
        match self.exchange_at_deadline(deadline, now, receive, |deadline, reply| {
            Job::Description {
                input,
                deadline,
                reply,
            }
        }) {
            Ok(description) => match description.disposition() {
                SourceDescriptionDisposition::CompleteSyntax => {
                    GuardedSourceDescription::complete(description)
                }
                SourceDescriptionDisposition::Fallback(reason) => {
                    GuardedSourceDescription::fallback(GuardFallback::Description(reason))
                }
            },
            Err(reason) => GuardedSourceDescription::fallback(reason),
        }
    }

    fn analyze_with(
        &self,
        script: CompleteScript,
        wait: Duration,
        now: impl FnMut() -> Instant,
        receive: impl FnOnce(
            &Receiver<Completed<Response>>,
            Duration,
        ) -> Result<Completed<Response>, RecvTimeoutError>,
    ) -> GuardedSourceActivity {
        if let Some(reason) = reject(&script) {
            return GuardedSourceActivity::fallback(reason);
        }
        match self.exchange(wait, now, receive, |deadline, reply| Job::Script {
            script,
            deadline,
            reply,
        }) {
            Ok(activity) => match activity.disposition() {
                PresentationDisposition::Eligible => GuardedSourceActivity::eligible(activity),
                PresentationDisposition::Fallback(reason) => {
                    GuardedSourceActivity::fallback(GuardFallback::Source(reason))
                }
            },
            Err(reason) => GuardedSourceActivity::fallback(reason),
        }
    }

    fn analyze_orchestration_with(
        &self,
        input: CompleteOrchestration,
        wait: Duration,
        now: impl FnMut() -> Instant,
        receive: impl FnOnce(
            &Receiver<Completed<worker::EnvelopeResponse>>,
            Duration,
        ) -> Result<Completed<worker::EnvelopeResponse>, RecvTimeoutError>,
    ) -> GuardedOrchestrationActivity {
        if !input.environments_known() {
            return GuardedOrchestrationActivity::fallback(GuardFallback::EnvironmentUnknown);
        }
        if input.source().len() > GUARD_MAX_SOURCE_BYTES
            || input.call_id().as_str().len() > GUARD_MAX_CALL_ID_BYTES
        {
            return GuardedOrchestrationActivity::fallback(GuardFallback::RequestTooLarge);
        }
        match self.exchange(wait, now, receive, |deadline, reply| Job::Orchestration {
            input,
            deadline,
            reply,
        }) {
            Ok(activity) => match activity.disposition() {
                OrchestrationDisposition::Eligible => {
                    GuardedOrchestrationActivity::eligible(activity)
                }
                OrchestrationDisposition::Fallback(reason) => {
                    GuardedOrchestrationActivity::fallback(GuardFallback::Orchestration(reason))
                }
            },
            Err(reason) => GuardedOrchestrationActivity::fallback(reason),
        }
    }

    fn exchange<T>(
        &self,
        wait: Duration,
        mut now: impl FnMut() -> Instant,
        receive: impl FnOnce(
            &Receiver<Completed<Result<T, GuardFallback>>>,
            Duration,
        ) -> Result<Completed<Result<T, GuardFallback>>, RecvTimeoutError>,
        job: impl FnOnce(Instant, SyncSender<Completed<Result<T, GuardFallback>>>) -> Job,
    ) -> Result<T, GuardFallback> {
        let wait = wait.min(GUARD_MAX_WAIT);
        if wait.is_zero() {
            return Err(GuardFallback::Deadline);
        }
        // Scheduling after admission must never renew the deadline.
        let deadline = now().checked_add(wait).ok_or(GuardFallback::Deadline)?;
        self.exchange_at_deadline(deadline, now, receive, job)
    }

    fn exchange_at_deadline<T>(
        &self,
        deadline: Instant,
        mut now: impl FnMut() -> Instant,
        receive: impl FnOnce(
            &Receiver<Completed<Result<T, GuardFallback>>>,
            Duration,
        ) -> Result<Completed<Result<T, GuardFallback>>, RecvTimeoutError>,
        job: impl FnOnce(Instant, SyncSender<Completed<Result<T, GuardFallback>>>) -> Job,
    ) -> Result<T, GuardFallback> {
        self.gate.admit()?;
        let (reply, receiver) = sync_channel(1);
        if self.jobs.try_send(job(deadline, reply)).is_err() {
            self.gate.disable();
            return Err(GuardFallback::WorkerUnavailable);
        }
        let remaining = deadline
            .checked_duration_since(now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or(GuardFallback::Deadline)?;
        let activity = receive(&receiver, remaining).map_err(|error| match error {
            RecvTimeoutError::Timeout => GuardFallback::Deadline,
            RecvTimeoutError::Disconnected => GuardFallback::WorkerUnavailable,
        })?;
        activity.at_deadline(deadline, now())
    }
}

fn reject(script: &CompleteScript) -> Option<GuardFallback> {
    if script.environment != EnvironmentAssumptions::FreshProcess {
        return Some(GuardFallback::EnvironmentUnknown);
    }
    let request = &script.request;
    if !request.snapshots.is_empty() {
        return Some(GuardFallback::SnapshotsUnsupported);
    }
    if request.source.len() > GUARD_MAX_SOURCE_BYTES
        || request.call_id.as_str().len() > GUARD_MAX_CALL_ID_BYTES
        || request
            .cwd
            .as_ref()
            .is_some_and(|cwd| cwd.len() > GUARD_MAX_CWD_BYTES)
    {
        return Some(GuardFallback::RequestTooLarge);
    }
    None
}

#[cfg(test)]
#[path = "source_guard_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "source_guard_deadline_tests.rs"]
mod deadline_tests;

#[cfg(test)]
#[path = "source_guard_orchestration_tests.rs"]
mod orchestration_tests;

#[cfg(test)]
#[path = "source_guard_description_tests.rs"]
mod description_tests;

#[cfg(test)]
#[path = "source_guard_selective_tests.rs"]
mod selective_tests;

#[cfg(test)]
#[path = "source_guard_presentation_tests.rs"]
mod presentation_tests;
