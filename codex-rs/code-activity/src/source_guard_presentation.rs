//! One admitted job supplies both conditional enrichment and its syntax fallback.

use super::GUARD_MAX_WAIT;
use super::SourceAnalysisGuard;
use super::worker::BudgetRequest;
use super::worker::Completed;
use super::worker::Job;
use super::worker::PresentationResponse;
use crate::ActivityError;
use crate::CompleteSourceDescriptionInput;
use crate::GuardFallback;
use crate::GuardedSourceDescription;
use crate::PythonWriteEndpoint;
use crate::SelectiveEditAnalysis;
use crate::SelectiveEditInput;
use crate::SourceDescription;
use crate::SourceDescriptionDisposition;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;
use std::time::Instant;

/// Selects source enrichment, never execution or permission policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SourcePresentationMode {
    /// Keep complete source descriptions without conditional API assumptions.
    SourceOnly,
    /// Also analyze conditional recipes; the host checks expected tool bindings.
    ConditionalForecast,
}

/// Both analyses own identical source bytes, grammar and call identity.
#[derive(Debug)]
#[must_use]
pub struct SourcePresentationInput {
    description: CompleteSourceDescriptionInput,
    forecast: Option<SelectiveEditInput>,
}

impl SourcePresentationInput {
    /// Constructs one bounded job; fallback cannot analyze a different source.
    ///
    /// # Errors
    /// Returns [`ActivityError::SourceLimit`] for an oversized/invalid cwd anchor.
    pub fn new(
        description: CompleteSourceDescriptionInput,
        mode: SourcePresentationMode,
        cwd: Option<&str>,
    ) -> Result<Self, ActivityError> {
        let forecast = match mode {
            SourcePresentationMode::SourceOnly => None,
            SourcePresentationMode::ConditionalForecast => Some(
                SelectiveEditInput::from_source(
                    description.call_id().clone(),
                    description.language(),
                    description.source(),
                    cwd,
                    description.limits(),
                )?
                .with_python_write_endpoint(PythonWriteEndpoint::LogicalAcceptance),
            ),
        };
        Ok(Self {
            description,
            forecast,
        })
    }

    pub(super) fn into_parts(self) -> (CompleteSourceDescriptionInput, Option<SelectiveEditInput>) {
        (self.description, self.forecast)
    }
}

impl BudgetRequest for SourcePresentationInput {
    fn clamp_elapsed(&mut self, remaining: Duration) {
        self.description.clamp_elapsed(remaining);
        if let Some(forecast) = &mut self.forecast {
            forecast.clamp_elapsed(remaining);
        }
    }
}

/// Both candidates arrived within one original deadline and worker admission.
/// The host may reject forecast presentation and use the complete syntax result.
#[derive(Debug)]
#[non_exhaustive]
pub struct GuardedSourcePresentation {
    forecast: Option<SelectiveEditAnalysis>,
    description: GuardedSourceDescription,
}

impl GuardedSourcePresentation {
    pub(super) fn new(
        forecast: Option<SelectiveEditAnalysis>,
        description: SourceDescription,
    ) -> Self {
        let description = match description.disposition() {
            SourceDescriptionDisposition::CompleteSyntax => {
                GuardedSourceDescription::complete(description)
            }
            SourceDescriptionDisposition::Fallback(reason) => {
                GuardedSourceDescription::fallback(GuardFallback::Description(reason))
            }
        };
        Self {
            forecast,
            description,
        }
    }

    /// Conditional candidate, still subject to disclosure and presentation checks.
    #[must_use]
    pub const fn forecast(&self) -> Option<&SelectiveEditAnalysis> {
        self.forecast.as_ref()
    }

    /// Original complete-source fallback, never an executed-effect record.
    #[must_use]
    pub const fn description(&self) -> &GuardedSourceDescription {
        &self.description
    }
}

impl SourceAnalysisGuard {
    /// Analyzes both presentation candidates without releasing/reacquiring admission.
    /// Scheduling, setup and both analyses consume the same capped deadline.
    ///
    /// # Errors
    /// Returns a [`GuardFallback`] for expired, busy or unavailable work; no
    /// candidate escapes a failed deadline, including a completed first analysis.
    pub fn present_until(
        &self,
        input: SourcePresentationInput,
        deadline: Instant,
    ) -> Result<GuardedSourcePresentation, GuardFallback> {
        self.present_until_with(input, deadline, Instant::now, Receiver::recv_timeout)
    }

    pub(super) fn present_until_with(
        &self,
        input: SourcePresentationInput,
        deadline: Instant,
        mut now: impl FnMut() -> Instant,
        receive: impl FnOnce(
            &Receiver<Completed<PresentationResponse>>,
            Duration,
        ) -> Result<Completed<PresentationResponse>, RecvTimeoutError>,
    ) -> Result<GuardedSourcePresentation, GuardFallback> {
        let current = now();
        let maximum = current
            .checked_add(GUARD_MAX_WAIT)
            .ok_or(GuardFallback::Deadline)?;
        let deadline = deadline.min(maximum);
        if deadline <= current {
            return Err(GuardFallback::Deadline);
        }
        self.exchange_at_deadline(deadline, now, receive, |deadline, reply| {
            Job::Presentation {
                input,
                deadline,
                reply,
            }
        })
    }
}
