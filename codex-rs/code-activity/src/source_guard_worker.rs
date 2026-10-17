//! Typed jobs share one worker, analyzer, and ownership-preserving reply cleanup.

use super::GuardedSourcePresentation;
use super::SourceAnalysisGuard;
use super::SourcePresentationInput;
use crate::Analyzer;
use crate::CompleteOrchestration;
use crate::CompleteScript;
use crate::CompleteSourceDescriptionInput;
use crate::GuardFallback;
use crate::OrchestrationActivity;
use crate::SelectiveEditAnalysis;
use crate::SelectiveEditInput;
use crate::SourceDescription;
use crate::WholeScriptActivity;
use std::sync::Arc;
use std::sync::atomic::AtomicU8;
use std::sync::atomic::Ordering;
use std::sync::mpsc::SyncSender;
use std::sync::mpsc::sync_channel;
use std::time::Instant;

#[path = "source_guard_budget.rs"]
mod budget;
#[path = "source_guard_reply.rs"]
mod reply;
pub(super) use budget::BudgetRequest;
use budget::budget;
pub(super) use budget::initialize_and_budget;
pub(super) use reply::Completed;
use reply::DisableOnExit;
use reply::PendingReply;

#[derive(Clone, Copy)]
#[repr(u8)]
pub(super) enum State {
    Ready,
    Active,
    Disabled,
}

#[derive(Debug)]
pub(super) struct Gate(pub(super) AtomicU8);

impl Gate {
    pub(super) fn admit(&self) -> Result<(), GuardFallback> {
        self.0
            .compare_exchange(
                State::Ready as u8,
                State::Active as u8,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .map(|_| ())
            .map_err(|state| {
                if state == State::Active as u8 {
                    GuardFallback::Busy
                } else {
                    GuardFallback::WorkerUnavailable
                }
            })
    }
    fn finish(&self) {
        let _ = self.0.compare_exchange(
            State::Active as u8,
            State::Ready as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
    pub(super) fn disable(&self) {
        self.0.store(State::Disabled as u8, Ordering::Release);
    }
}

pub(super) type Response = Result<WholeScriptActivity, GuardFallback>;
pub(super) type EnvelopeResponse = Result<OrchestrationActivity, GuardFallback>;

pub(super) type DescriptionResponse = Result<SourceDescription, GuardFallback>;
pub(super) type ForecastResponse = Result<SelectiveEditAnalysis, GuardFallback>;
pub(super) type PresentationResponse = Result<GuardedSourcePresentation, GuardFallback>;

pub(super) enum Job {
    Presentation {
        input: SourcePresentationInput,
        deadline: Instant,
        reply: SyncSender<Completed<PresentationResponse>>,
    },
    Forecast {
        input: SelectiveEditInput,
        deadline: Instant,
        reply: SyncSender<Completed<ForecastResponse>>,
    },
    Description {
        input: CompleteSourceDescriptionInput,
        deadline: Instant,
        reply: SyncSender<Completed<DescriptionResponse>>,
    },
    Script {
        script: CompleteScript,
        deadline: Instant,
        reply: SyncSender<Completed<Response>>,
    },
    Orchestration {
        input: CompleteOrchestration,
        deadline: Instant,
        reply: SyncSender<Completed<EnvelopeResponse>>,
    },
}

pub(super) trait Worker: Send + 'static {
    fn presentation(
        &mut self,
        input: SourcePresentationInput,
        deadline: Instant,
    ) -> PresentationResponse {
        let (description, forecast) = input.into_parts();
        let forecast = forecast
            .map(|input| self.forecast(input, deadline))
            .transpose()?;
        let description = self.description(description, deadline)?;
        Ok(GuardedSourcePresentation::new(forecast, description))
    }
    fn forecast(&mut self, _input: SelectiveEditInput, _deadline: Instant) -> ForecastResponse {
        Err(GuardFallback::WorkerUnavailable)
    }
    fn description(
        &mut self,
        input: CompleteSourceDescriptionInput,
        deadline: Instant,
    ) -> DescriptionResponse;
    fn script(&mut self, input: CompleteScript, deadline: Instant) -> Response;
    fn orchestration(
        &mut self,
        input: CompleteOrchestration,
        deadline: Instant,
    ) -> EnvelopeResponse;
}

#[derive(Default)]
pub(super) struct AnalyzerWorker {
    analyzer: Option<Analyzer>,
}

impl Worker for AnalyzerWorker {
    fn forecast(&mut self, input: SelectiveEditInput, deadline: Instant) -> ForecastResponse {
        let input = initialize_and_budget(
            input,
            deadline,
            &mut self.analyzer,
            || Analyzer::new().ok(),
            Instant::now,
        )?;
        let analyzer = self
            .analyzer
            .as_mut()
            .ok_or(GuardFallback::WorkerUnavailable)?;
        Ok(analyzer.forecast_edits(input))
    }

    fn description(
        &mut self,
        input: CompleteSourceDescriptionInput,
        deadline: Instant,
    ) -> DescriptionResponse {
        let input = initialize_and_budget(
            input,
            deadline,
            &mut self.analyzer,
            || Analyzer::new().ok(),
            Instant::now,
        )?;
        let analyzer = self
            .analyzer
            .as_mut()
            .ok_or(GuardFallback::WorkerUnavailable)?;
        Ok(analyzer.describe_complete(input))
    }

    fn script(&mut self, input: CompleteScript, deadline: Instant) -> Response {
        let input = initialize_and_budget(
            input,
            deadline,
            &mut self.analyzer,
            || Analyzer::new().ok(),
            Instant::now,
        )?;
        let analyzer = self
            .analyzer
            .as_mut()
            .ok_or(GuardFallback::WorkerUnavailable)?;
        Ok(analyzer.analyze_complete(input))
    }
    fn orchestration(
        &mut self,
        input: CompleteOrchestration,
        deadline: Instant,
    ) -> EnvelopeResponse {
        let input = initialize_and_budget(
            input,
            deadline,
            &mut self.analyzer,
            || Analyzer::new().ok(),
            Instant::now,
        )?;
        let analyzer = self
            .analyzer
            .as_mut()
            .ok_or(GuardFallback::WorkerUnavailable)?;
        Ok(analyzer.analyze_orchestration(input))
    }
}

fn run<R: BudgetRequest, T>(
    input: R,
    deadline: Instant,
    reply: SyncSender<Completed<Result<T, GuardFallback>>>,
    gate: &Arc<Gate>,
    now: Instant,
    work: impl FnOnce(R, Instant) -> Result<T, GuardFallback>,
) -> bool {
    let pending = PendingReply {
        reply: Some(reply),
        gate: Arc::clone(gate),
    };
    let activity = budget(input, deadline, now).and_then(|input| work(input, deadline));
    pending.finish(activity)
}

impl SourceAnalysisGuard {
    pub(super) fn with_job_worker(
        mut work: impl Worker,
        mut finished: impl FnMut() + Send + 'static,
        mut now: impl FnMut() -> Instant + Send + 'static,
    ) -> Self {
        let (jobs, receiver) = sync_channel::<Job>(1);
        let gate = Arc::new(Gate(AtomicU8::new(State::Ready as u8)));
        let worker_gate = Arc::clone(&gate);
        let spawned = std::thread::Builder::new()
            .name("source-analysis".into())
            .spawn(move || {
                let _disable = DisableOnExit(Arc::clone(&worker_gate));
                while let Ok(job) = receiver.recv() {
                    let failed = match job {
                        Job::Presentation {
                            input,
                            deadline,
                            reply,
                        } => run(
                            input,
                            deadline,
                            reply,
                            &worker_gate,
                            now(),
                            |input, deadline| work.presentation(input, deadline),
                        ),
                        Job::Forecast {
                            input,
                            deadline,
                            reply,
                        } => run(
                            input,
                            deadline,
                            reply,
                            &worker_gate,
                            now(),
                            |input, deadline| work.forecast(input, deadline),
                        ),
                        Job::Description {
                            input,
                            deadline,
                            reply,
                        } => run(
                            input,
                            deadline,
                            reply,
                            &worker_gate,
                            now(),
                            |input, deadline| work.description(input, deadline),
                        ),
                        Job::Script {
                            script,
                            deadline,
                            reply,
                        } => run(
                            script,
                            deadline,
                            reply,
                            &worker_gate,
                            now(),
                            |input, deadline| work.script(input, deadline),
                        ),
                        Job::Orchestration {
                            input,
                            deadline,
                            reply,
                        } => run(
                            input,
                            deadline,
                            reply,
                            &worker_gate,
                            now(),
                            |input, deadline| work.orchestration(input, deadline),
                        ),
                    };
                    finished();
                    if failed {
                        break;
                    }
                }
            });
        match spawned {
            Ok(handle) => drop(handle),
            Err(_) => gate.disable(),
        }
        Self { jobs, gate }
    }
}

#[cfg(test)]
#[path = "source_guard_worker_test_support.rs"]
mod test_support;
