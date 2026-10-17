//! Read-only outcomes from the shared source-analysis worker.

use crate::FallbackReason;
use crate::FileDiff;
use crate::OrchestrationActivity;
use crate::OrchestrationFallback;
use crate::OrchestrationGroup;
use crate::SourceDescription;
use crate::SourceDescriptionFallback;
use crate::UiRow;
use crate::WholeScriptActivity;
use serde::Serialize;

/// Why guarded enrichment was discarded; retain the host's original command UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum GuardFallback {
    /// The host did not establish the required fresh, ordinary environment.
    EnvironmentUnknown,
    /// Source, call identity, or working-directory logical bytes exceeded a cap.
    RequestTooLarge,
    /// Snapshots are outside the source-only worker's contract.
    SnapshotsUnsupported,
    /// The single admission remains occupied, including timed-out work.
    Busy,
    /// The admission deadline expired or the requested duration was zero.
    Deadline,
    /// Worker startup, response delivery, or analysis failed; no restart occurs.
    WorkerUnavailable,
    /// Complete-source semantic gating rejected the result.
    Source(FallbackReason),
    /// Whole-envelope semantic gating rejected all child presentation.
    Orchestration(OrchestrationFallback),
    /// Whole-source syntax description was unsupported; no references survive.
    Description(SourceDescriptionFallback),
}

/// Eligibility of a guarded complete-source result, never execution authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type", content = "reason", rename_all = "camelCase")]
#[non_exhaustive]
pub enum GuardDisposition {
    /// The complete result arrived before its admission deadline and is eligible.
    Eligible,
    /// No rows or diffs survive this outcome.
    Fallback(GuardFallback),
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
enum Outcome<T> {
    Eligible(T),
    Fallback(GuardFallback),
}

/// Read-only eligible activity or an explicit fallback without partial evidence.
#[derive(Debug, Serialize)]
#[serde(transparent)]
#[non_exhaustive]
pub struct GuardedSourceActivity {
    outcome: Outcome<WholeScriptActivity>,
}

impl GuardedSourceActivity {
    pub(super) fn fallback(reason: GuardFallback) -> Self {
        Self {
            outcome: Outcome::Fallback(reason),
        }
    }

    pub(super) fn eligible(activity: WholeScriptActivity) -> Self {
        Self {
            outcome: Outcome::Eligible(activity),
        }
    }

    /// Returns complete-source eligibility or a typed fallback reason.
    #[must_use]
    pub const fn disposition(&self) -> GuardDisposition {
        match &self.outcome {
            Outcome::Eligible(_) => GuardDisposition::Eligible,
            Outcome::Fallback(reason) => GuardDisposition::Fallback(*reason),
        }
    }

    /// Borrows only eligible activity; fallback has no report escape hatch.
    #[must_use]
    pub const fn eligible_activity(&self) -> Option<&WholeScriptActivity> {
        match &self.outcome {
            Outcome::Eligible(activity) => Some(activity),
            Outcome::Fallback(_) => None,
        }
    }

    /// Static-intent rows; empty on every guard or semantic fallback.
    #[must_use]
    pub fn ui_rows(&self) -> Vec<UiRow<'_>> {
        self.eligible_activity()
            .map_or_else(Vec::new, WholeScriptActivity::ui_rows)
    }

    /// Read-only diffs; source-only requests never supply snapshot comparisons.
    #[must_use]
    pub fn diffs(&self) -> &[FileDiff] {
        self.eligible_activity()
            .map_or(&[], WholeScriptActivity::diffs)
    }
}

/// Immutable guarded whole-envelope activity; fallback retains no partial groups.
/// This describes provisional intent only and never grants execution authority.
#[derive(Debug, Serialize)]
#[serde(transparent)]
#[non_exhaustive]
pub struct GuardedOrchestrationActivity {
    outcome: Outcome<OrchestrationActivity>,
}

impl GuardedOrchestrationActivity {
    pub(super) fn fallback(reason: GuardFallback) -> Self {
        Self {
            outcome: Outcome::Fallback(reason),
        }
    }

    pub(super) fn eligible(activity: OrchestrationActivity) -> Self {
        Self {
            outcome: Outcome::Eligible(activity),
        }
    }

    /// Returns complete-envelope eligibility or a typed guard/semantic fallback.
    #[must_use]
    pub const fn disposition(&self) -> GuardDisposition {
        match &self.outcome {
            Outcome::Eligible(_) => GuardDisposition::Eligible,
            Outcome::Fallback(reason) => GuardDisposition::Fallback(*reason),
        }
    }

    /// Borrows eligible complete activity; fallback has no report escape hatch.
    #[must_use]
    pub const fn eligible_activity(&self) -> Option<&OrchestrationActivity> {
        match &self.outcome {
            Outcome::Eligible(activity) => Some(activity),
            Outcome::Fallback(_) => None,
        }
    }

    /// Source-level groups with child-local rows, empty on every fallback.
    #[must_use]
    pub fn groups(&self) -> &[OrchestrationGroup] {
        self.eligible_activity()
            .map_or(&[], OrchestrationActivity::groups)
    }
}

/// Whether a guarded source description completed before its original deadline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type", content = "reason", rename_all = "camelCase")]
#[non_exhaustive]
pub enum SourceDescriptionGuardDisposition {
    /// Complete supported syntax, with no statement about runtime effects.
    CompleteSyntax,
    /// No source description survived the guard or closed grammar.
    Fallback(GuardFallback),
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
enum DescriptionOutcome {
    CompleteSyntax(SourceDescription),
    Fallback(GuardFallback),
}

/// Guarded complete syntax description, separate from runtime-effect activity.
/// A successful result describes source references only and has no UI effect rows.
#[derive(Debug, Serialize)]
#[serde(transparent)]
#[non_exhaustive]
pub struct GuardedSourceDescription {
    outcome: DescriptionOutcome,
}

impl GuardedSourceDescription {
    pub(super) fn fallback(reason: GuardFallback) -> Self {
        Self {
            outcome: DescriptionOutcome::Fallback(reason),
        }
    }

    pub(super) fn complete(description: SourceDescription) -> Self {
        Self {
            outcome: DescriptionOutcome::CompleteSyntax(description),
        }
    }

    /// Complete syntax arrived before its deadline, or the entire result is absent.
    /// Completeness never establishes runtime effects or execution authority.
    #[must_use]
    pub const fn disposition(&self) -> SourceDescriptionGuardDisposition {
        match &self.outcome {
            DescriptionOutcome::CompleteSyntax(_) => {
                SourceDescriptionGuardDisposition::CompleteSyntax
            }
            DescriptionOutcome::Fallback(reason) => {
                SourceDescriptionGuardDisposition::Fallback(*reason)
            }
        }
    }

    /// Borrows the complete source description; no partial result escapes fallback.
    #[must_use]
    pub const fn complete_description(&self) -> Option<&SourceDescription> {
        match &self.outcome {
            DescriptionOutcome::CompleteSyntax(description) => Some(description),
            DescriptionOutcome::Fallback(_) => None,
        }
    }
}
