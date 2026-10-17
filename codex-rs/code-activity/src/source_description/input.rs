//! Complete source storage without execution assumptions or snapshots.

use crate::ActivityError;
use crate::AnalysisLimits;
use crate::CallId;
use crate::GUARD_MAX_CALL_ID_BYTES;
use crate::GUARD_MAX_SOURCE_BYTES;
use crate::Language;
use std::time::Duration;

/// A complete source to describe, with no assertion about its runtime.
/// Construction asserts completeness only. Hosts retain the original command.
#[derive(Debug)]
#[must_use]
pub struct CompleteSourceDescriptionInput {
    pub(crate) call_id: CallId,
    pub(crate) language: Language,
    pub(crate) source: String,
    pub(crate) limits: AnalysisLimits,
}

impl CompleteSourceDescriptionInput {
    /// Copies a complete source only after checking logical byte limits.
    /// No runtime, filesystem or permission assertion is accepted here.
    ///
    /// # Errors
    /// Returns [`ActivityError::SourceLimit`] for an oversized source or call ID.
    pub fn from_source(
        call_id: CallId,
        language: Language,
        source: &str,
        limits: AnalysisLimits,
    ) -> Result<Self, ActivityError> {
        if source.len() > limits.source_bytes().min(GUARD_MAX_SOURCE_BYTES)
            || call_id.as_str().len() > GUARD_MAX_CALL_ID_BYTES
        {
            return Err(ActivityError::SourceLimit);
        }
        Ok(Self {
            call_id,
            language,
            source: source.to_owned(),
            limits,
        })
    }

    /// Original opaque call identity.
    #[must_use]
    pub const fn call_id(&self) -> &CallId {
        &self.call_id
    }

    /// Exact decoded complete source.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Requested source grammar, not a verified runtime identity.
    #[must_use]
    pub const fn language(&self) -> Language {
        self.language
    }

    /// Logical source limit and cooperative elapsed-time budget.
    #[must_use]
    pub const fn limits(&self) -> AnalysisLimits {
        self.limits
    }

    pub(crate) fn clamp_elapsed(&mut self, remaining: Duration) {
        self.limits = AnalysisLimits::new(
            self.limits.source_bytes(),
            self.limits.elapsed().min(remaining),
        );
    }
}
