//! Forecast-only requests retain the same shared admission and deadline.

use super::GUARD_MAX_WAIT;
use super::SourceAnalysisGuard;
use super::worker;
use super::worker::Completed;
use super::worker::Job;
use crate::GuardFallback;
use crate::SelectiveEditAnalysis;
use crate::SelectiveEditInput;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;
use std::time::Instant;

impl SourceAnalysisGuard {
    /// Forecasts conditional edits on the same worker and original deadline.
    /// This does not attest runtime bindings or change execution permissions.
    /// Unavailable native results remain explicit and contain no predictions.
    pub fn forecast_until(
        &self,
        input: SelectiveEditInput,
        deadline: Instant,
    ) -> Result<SelectiveEditAnalysis, GuardFallback> {
        self.forecast_until_with(input, deadline, Instant::now, Receiver::recv_timeout)
    }

    pub(super) fn forecast_until_with(
        &self,
        input: SelectiveEditInput,
        deadline: Instant,
        mut now: impl FnMut() -> Instant,
        receive: impl FnOnce(
            &Receiver<Completed<worker::ForecastResponse>>,
            Duration,
        ) -> Result<Completed<worker::ForecastResponse>, RecvTimeoutError>,
    ) -> Result<SelectiveEditAnalysis, GuardFallback> {
        let current = now();
        let maximum = current
            .checked_add(GUARD_MAX_WAIT)
            .ok_or(GuardFallback::Deadline)?;
        let deadline = deadline.min(maximum);
        if deadline <= current {
            return Err(GuardFallback::Deadline);
        }
        self.exchange_at_deadline(deadline, now, receive, |deadline, reply| Job::Forecast {
            input,
            deadline,
            reply,
        })
    }
}
