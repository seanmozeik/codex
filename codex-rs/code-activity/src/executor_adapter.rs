//! Local prepare-before-dispatch prototype; no production executor is activated.

#[path = "executor_adapter_context.rs"]
mod context;
pub use context::ExecutorContext;
pub use context::ExecutorRuntimeProfile;
pub use context::ExecutorSource;
pub use context::FinalExecutorInput;

use crate::AnalysisLimits;
use crate::CompleteOrchestration;
use crate::CompleteScript;
use crate::EnvironmentAssumptions;
use crate::GUARD_MAX_CALL_ID_BYTES;
use crate::GUARD_MAX_CWD_BYTES;
use crate::GUARD_MAX_SOURCE_BYTES;
use crate::GuardDisposition;
use crate::GuardFallback;
use crate::GuardedOrchestrationActivity;
use crate::GuardedSourceActivity;
use crate::Language;
use crate::OrchestrationGroup;
use crate::PythonRuntimeProfile;
use crate::Request;
use crate::SourceAnalysisGuard;
use crate::UiRow;
use serde::Serialize;
use std::time::Duration;

/// Atomic source-only presentation; no execution decision or observed snapshots.
#[derive(Debug, Serialize)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
#[non_exhaustive]
pub enum ExecutorPresentation {
    /// Complete final script intent, possibly a whole-source guard fallback.
    Script(GuardedSourceActivity),
    /// Whole-envelope intent; children still require final post-hook analysis.
    Orchestration(GuardedOrchestrationActivity),
    /// Input/context rejection before submitting any worker job.
    Fallback(GuardFallback),
}

impl ExecutorPresentation {
    /// Rich-display eligibility only; this never authorizes or blocks dispatch.
    #[must_use]
    pub fn disposition(&self) -> GuardDisposition {
        match self {
            Self::Script(activity) => activity.disposition(),
            Self::Orchestration(activity) => activity.disposition(),
            Self::Fallback(reason) => GuardDisposition::Fallback(*reason),
        }
    }

    /// Final script rows, empty for fallback and orchestration (which has groups).
    #[must_use]
    pub fn script_rows(&self) -> Vec<UiRow<'_>> {
        match self {
            Self::Script(activity) => activity.ui_rows(),
            _ => Vec::new(),
        }
    }

    /// Complete provisional envelope groups, empty on every fallback.
    /// The existing envelope contract leaves child cwd unresolved.
    #[must_use]
    pub fn groups(&self) -> &[OrchestrationGroup] {
        match self {
            Self::Orchestration(activity) => activity.groups(),
            _ => &[],
        }
    }
}

/// Final presentation prepared synchronously before calling existing dispatch.
/// There is no mutable input or partial-result escape hatch. Source snapshots,
/// execution status and permission decisions are deliberately absent.
#[derive(Debug)]
#[must_use]
pub struct PreparedExecutor<'a, L> {
    input: FinalExecutorInput<'a, L>,
    presentation: ExecutorPresentation,
}

impl<'a, L> FinalExecutorInput<'a, L> {
    /// Consumes complete input and waits for the shared guard's final decision.
    /// Pre-copy byte checks cover source, identities and cwd. Fallback preserves
    /// the same original input for dispatch. This does not execute source or
    /// verify host assertions, and it never supplies observed snapshots.
    ///
    /// The guard's capped response wait does not preempt worker CPU work or bound
    /// total RSS. See [`SourceAnalysisGuard`] for admission and failure behavior.
    pub fn prepare(self, limits: AnalysisLimits, wait: Duration) -> PreparedExecutor<'a, L> {
        let presentation = prepare_source(&self, limits, wait);
        PreparedExecutor {
            input: self,
            presentation,
        }
    }
}

impl<'a, L> PreparedExecutor<'a, L> {
    /// Same original launch and exact source/cwd/context supplied by the host.
    #[must_use]
    pub const fn input(&self) -> &FinalExecutorInput<'a, L> {
        &self.input
    }

    /// Final complete-source decision available before dispatch.
    #[must_use]
    pub const fn presentation(&self) -> &ExecutorPresentation {
        &self.presentation
    }

    /// Calls the existing host callback exactly once, on every presentation
    /// outcome, passing the unchanged input. No authorization path is replaced.
    /// Its return value, including an error or future, is preserved unchanged.
    /// A returned future is not polled here; return is not proof of execution.
    ///
    /// A preloaded-pathlib runner first starts its trusted bootstrap, verifies
    /// readiness, prepares this result, then releases this same source in the
    /// callback. Bootstrap effects are outside analyzed user-source intent.
    pub fn dispatch<O>(
        self,
        dispatch: impl FnOnce(&FinalExecutorInput<'a, L>) -> O,
    ) -> ExecutorDispatch<'a, L, O> {
        let outcome = dispatch(&self.input);
        ExecutorDispatch {
            input: self.input,
            presentation: self.presentation,
            outcome,
        }
    }
}

/// Opaque callback outcome alongside the immutable prepared presentation.
/// This is not a completed-process record, even when the callback returned Ok.
#[derive(Debug)]
#[must_use]
pub struct ExecutorDispatch<'a, L, O> {
    input: FinalExecutorInput<'a, L>,
    presentation: ExecutorPresentation,
    outcome: O,
}

impl<'a, L, O> ExecutorDispatch<'a, L, O> {
    /// Same launch object and source/context view passed to the host callback.
    #[must_use]
    pub const fn input(&self) -> &FinalExecutorInput<'a, L> {
        &self.input
    }

    /// Static intent only; callback results never upgrade this decision.
    #[must_use]
    pub const fn presentation(&self) -> &ExecutorPresentation {
        &self.presentation
    }

    /// Unmodified host callback return value; no success interpretation occurs.
    #[must_use]
    pub const fn outcome(&self) -> &O {
        &self.outcome
    }

    /// Returns the presentation and exact callback result without observing files.
    #[must_use]
    pub fn into_parts(self) -> (ExecutorPresentation, O) {
        (self.presentation, self.outcome)
    }
}

fn prepare_source<L>(
    input: &FinalExecutorInput<'_, L>,
    limits: AnalysisLimits,
    wait: Duration,
) -> ExecutorPresentation {
    let context = input.context();
    if input.source().source().len() > limits.source_bytes().min(GUARD_MAX_SOURCE_BYTES)
        || [
            input.call_id(),
            context.launch_id(),
            context.readiness_identity(),
        ]
        .iter()
        .any(|id| id.len() > GUARD_MAX_CALL_ID_BYTES)
        || input
            .cwd()
            .is_some_and(|cwd| cwd.len() > GUARD_MAX_CWD_BYTES)
    {
        return ExecutorPresentation::Fallback(GuardFallback::RequestTooLarge);
    }
    if context.profile() == ExecutorRuntimeProfile::Unknown
        || context.launch_id().is_empty()
        || context.readiness_identity().is_empty()
    {
        return ExecutorPresentation::Fallback(GuardFallback::EnvironmentUnknown);
    }
    let limits = AnalysisLimits::new(
        limits.source_bytes().min(GUARD_MAX_SOURCE_BYTES),
        limits.elapsed(),
    );
    match input.source() {
        ExecutorSource::Script { language, source } => {
            if context.profile() == ExecutorRuntimeProfile::PreloadedPathlib
                && language != Language::Python
            {
                return ExecutorPresentation::Fallback(GuardFallback::EnvironmentUnknown);
            }
            let request = Request::new(
                input.call_id().into(),
                language,
                source.into(),
                input.cwd().map(str::to_owned),
            );
            let Ok(mut complete) =
                CompleteScript::from_request(request, EnvironmentAssumptions::FreshProcess, limits)
            else {
                return ExecutorPresentation::Fallback(GuardFallback::RequestTooLarge);
            };
            if context.profile() == ExecutorRuntimeProfile::PreloadedPathlib {
                complete = complete
                    .with_python_runtime_profile(PythonRuntimeProfile::PreloadedTrustedPathlib);
            }
            ExecutorPresentation::Script(SourceAnalysisGuard::shared().analyze(complete, wait))
        }
        ExecutorSource::Orchestration { source } => {
            if context.profile() != ExecutorRuntimeProfile::OrdinaryFresh {
                return ExecutorPresentation::Fallback(GuardFallback::EnvironmentUnknown);
            }
            let Ok(complete) = CompleteOrchestration::from_source(
                input.call_id().into(),
                source,
                context.bindings(),
                EnvironmentAssumptions::FreshProcess,
                limits,
            ) else {
                return ExecutorPresentation::Fallback(GuardFallback::RequestTooLarge);
            };
            ExecutorPresentation::Orchestration(
                SourceAnalysisGuard::shared().analyze_orchestration(complete, wait),
            )
        }
    }
}
