//! Cooperative budgets are reduced, never renewed, by queue and setup time.

use crate::AnalysisLimits;
use crate::CompleteOrchestration;
use crate::CompleteScript;
use crate::CompleteSourceDescriptionInput;
use crate::GuardFallback;
use crate::SelectiveEditInput;
use std::time::Duration;
use std::time::Instant;

pub(in crate::source_guard) trait BudgetRequest: Sized {
    fn clamp_elapsed(&mut self, remaining: Duration);
}

impl BudgetRequest for CompleteScript {
    fn clamp_elapsed(&mut self, remaining: Duration) {
        self.limits = AnalysisLimits::new(
            self.limits.source_bytes(),
            self.limits.elapsed().min(remaining),
        );
    }
}

impl BudgetRequest for CompleteOrchestration {
    fn clamp_elapsed(&mut self, remaining: Duration) {
        CompleteOrchestration::clamp_elapsed(self, remaining);
    }
}

impl BudgetRequest for CompleteSourceDescriptionInput {
    fn clamp_elapsed(&mut self, remaining: Duration) {
        CompleteSourceDescriptionInput::clamp_elapsed(self, remaining);
    }
}

impl BudgetRequest for SelectiveEditInput {
    fn clamp_elapsed(&mut self, remaining: Duration) {
        SelectiveEditInput::clamp_elapsed(self, remaining);
    }
}

pub(in crate::source_guard) fn budget<R: BudgetRequest>(
    mut input: R,
    deadline: Instant,
    now: Instant,
) -> Result<R, GuardFallback> {
    let remaining = deadline
        .checked_duration_since(now)
        .filter(|remaining| !remaining.is_zero())
        .ok_or(GuardFallback::Deadline)?;
    input.clamp_elapsed(remaining);
    Ok(input)
}

pub(in crate::source_guard) fn initialize_and_budget<R: BudgetRequest, T>(
    input: R,
    deadline: Instant,
    analyzer: &mut Option<T>,
    initialize: impl FnOnce() -> Option<T>,
    mut now: impl FnMut() -> Instant,
) -> Result<R, GuardFallback> {
    let input = budget(input, deadline, now())?;
    if analyzer.is_none() {
        *analyzer = Some(initialize().ok_or(GuardFallback::WorkerUnavailable)?);
    }
    budget(input, deadline, now())
}
