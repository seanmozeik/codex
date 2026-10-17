//! Consumed complete-source boundary, separate from command execution items.

use crate::AnalysisLimits;
use crate::CallId;
use crate::EnvironmentAssumptions;
use std::time::Duration;

pub(super) const MAX_SOURCE: usize = 32 * 1024;
pub(super) const MAX_VALUES: usize = 8;
pub(super) const MAX_TEXT: usize = 16_320;

/// Host assertions about the outer JavaScript execution environment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum OrchestratorBindings {
    /// A fresh lexical environment with native, unmodified `tools.exec_command`
    /// and `Promise.all`/`allSettled`. Source parsing cannot establish this.
    TrustedFreshHostBindings,
    /// Persistent, modified or otherwise unverified outer bindings.
    Unknown,
}

/// Failure to retain complete bounded orchestration input.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum OrchestrationInputError {
    /// Parent call identity exceeds 256 UTF-8 bytes.
    #[error("Orchestration call identity exceeds its byte limit")]
    CallIdentityLimit,
    /// Decoded source exceeded its configured limit, at most 32 KiB.
    #[error("Orchestration source exceeds its byte limit")]
    SourceLimit,
    /// An earlier overflow invalidated the entire retained prefix.
    #[error("Orchestration input was aborted")]
    Aborted,
}

/// Storage that cannot be analyzed until the host marks the input complete.
/// Appending never parses, dispatches, or produces provisional effects.
///
/// ```compile_fail
/// use codex_code_activity::{Analyzer, OrchestrationBuffer};
/// fn premature(analyzer: &mut Analyzer, buffer: OrchestrationBuffer) {
///     analyzer.analyze_orchestration(buffer);
/// }
/// ```
#[derive(Debug)]
#[must_use]
pub struct OrchestrationBuffer {
    complete: CompleteOrchestration,
    aborted: bool,
}

/// Immutable complete custom-tool source, consumed by orchestration analysis.
/// Completeness and environment assertions confer no execution permission.
#[derive(Debug)]
#[must_use]
pub struct CompleteOrchestration {
    pub(super) call_id: CallId,
    pub(super) source: String,
    pub(super) bindings: OrchestratorBindings,
    pub(super) environment: EnvironmentAssumptions,
    pub(super) limits: AnalysisLimits,
}

impl OrchestrationBuffer {
    /// Starts empty storage with separate outer and child runtime assumptions.
    /// No cwd or snapshots are accepted; target cwd stays unresolved.
    ///
    /// # Errors
    /// Returns [`OrchestrationInputError::CallIdentityLimit`] above 256 ID bytes.
    pub fn new(
        call_id: CallId,
        bindings: OrchestratorBindings,
        environment: EnvironmentAssumptions,
        limits: AnalysisLimits,
    ) -> Result<Self, OrchestrationInputError> {
        if call_id.as_str().len() > 256 {
            return Err(OrchestrationInputError::CallIdentityLimit);
        }
        Ok(Self {
            complete: CompleteOrchestration {
                call_id,
                source: String::new(),
                bindings,
                environment,
                limits: AnalysisLimits::new(
                    limits.source_bytes().min(MAX_SOURCE),
                    limits.elapsed(),
                ),
            },
            aborted: false,
        })
    }

    /// Retains decoded input without exposing a partially analyzed prefix.
    ///
    /// # Errors
    /// Overflow discards the prefix and returns `SourceLimit`; later appends
    /// return `Aborted`. Caller-owned transport allocations are outside this cap.
    pub fn append(&mut self, fragment: &str) -> Result<(), OrchestrationInputError> {
        if self.aborted {
            return Err(OrchestrationInputError::Aborted);
        }
        if fragment.len()
            > self
                .complete
                .limits
                .source_bytes()
                .saturating_sub(self.complete.source.len())
        {
            self.complete.source = String::new();
            self.aborted = true;
            return Err(OrchestrationInputError::SourceLimit);
        }
        self.complete.source.push_str(fragment);
        Ok(())
    }

    /// Consumes storage when the exact final decoded input has arrived.
    ///
    /// # Errors
    /// Returns `Aborted` after any source overflow.
    pub fn finish(self) -> Result<CompleteOrchestration, OrchestrationInputError> {
        if self.aborted {
            Err(OrchestrationInputError::Aborted)
        } else {
            Ok(self.complete)
        }
    }
}

impl CompleteOrchestration {
    pub(crate) fn limits(&self) -> AnalysisLimits {
        self.limits
    }

    pub(crate) fn clamp_elapsed(&mut self, elapsed: Duration) {
        self.limits = AnalysisLimits::new(
            self.limits.source_bytes(),
            self.limits.elapsed().min(elapsed),
        );
    }

    pub(crate) fn environments_known(&self) -> bool {
        self.bindings == OrchestratorBindings::TrustedFreshHostBindings
            && self.environment == EnvironmentAssumptions::FreshProcess
    }

    /// Original host call identity, bounded to 256 UTF-8 bytes.
    #[must_use]
    pub fn call_id(&self) -> &CallId {
        &self.call_id
    }

    /// Copies already-complete bounded source; use the buffer for streamed input.
    ///
    /// # Errors
    /// Returns the same identity/source errors as [`OrchestrationBuffer`].
    pub fn from_source(
        call_id: CallId,
        source: &str,
        bindings: OrchestratorBindings,
        environment: EnvironmentAssumptions,
        limits: AnalysisLimits,
    ) -> Result<Self, OrchestrationInputError> {
        let mut buffer = OrchestrationBuffer::new(call_id, bindings, environment, limits)?;
        buffer.append(source)?;
        buffer.finish()
    }

    /// Exact retained custom-tool source; analysis never rewrites dispatch input.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }
}
