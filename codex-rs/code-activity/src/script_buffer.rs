//! Complete-input boundary for the strict presentation API.

use crate::ActivityError;
use crate::AnalysisLimits;
use crate::CallId;
use crate::EnvironmentAssumptions;
use crate::Language;
use crate::PythonRuntimeProfile;
use crate::Request;

/// Bounded decoded source storage. Appending never parses or emits activity.
///
/// The host retains the original command for fallback and owns transport decoding,
/// cancellation, and deciding when final post-hook source has arrived.
///
/// ```compile_fail
/// use codex_code_activity::{Analyzer, ScriptBuffer};
/// fn premature(analyzer: &mut Analyzer, buffer: ScriptBuffer) {
///     analyzer.analyze_complete(buffer);
/// }
/// ```
#[derive(Debug)]
#[must_use]
pub struct ScriptBuffer {
    request: Request,
    environment: EnvironmentAssumptions,
    limits: AnalysisLimits,
    aborted: bool,
}

/// Immutable complete input, consumed by [`crate::Analyzer::analyze_complete`].
///
/// Construction asserts input completeness, not supported semantics or permission.
#[derive(Debug)]
#[must_use]
pub struct CompleteScript {
    pub(crate) request: Request,
    pub(crate) environment: EnvironmentAssumptions,
    pub(crate) limits: AnalysisLimits,
    pub(crate) python_runtime_profile: PythonRuntimeProfile,
}

impl ScriptBuffer {
    /// Starts empty storage with explicit execution assumptions and source limits.
    pub fn new(
        call_id: CallId,
        language: Language,
        cwd: Option<String>,
        environment: EnvironmentAssumptions,
        limits: AnalysisLimits,
    ) -> Self {
        Self {
            request: Request::new(call_id, language, String::new(), cwd),
            environment,
            limits,
            aborted: false,
        }
    }

    /// Stores a decoded fragment without producing a provisional report.
    ///
    /// # Errors
    /// Returns [`ActivityError::SourceLimit`] when the configured byte limit would
    /// be exceeded, and [`ActivityError::StreamAborted`] on subsequent appends.
    /// Overflow drops the entire retained prefix; it can never be analyzed later.
    pub fn append(&mut self, fragment: &str) -> Result<(), ActivityError> {
        if self.aborted {
            return Err(ActivityError::StreamAborted);
        }
        if fragment.len()
            > self
                .limits
                .source_bytes()
                .saturating_sub(self.request.source.len())
        {
            self.aborted = true;
            self.request.source = String::new();
            return Err(ActivityError::SourceLimit);
        }
        self.request.source.push_str(fragment);
        Ok(())
    }

    /// Consumes storage after the host has received the complete decoded source.
    ///
    /// # Errors
    /// Returns [`ActivityError::StreamAborted`] if any append exceeded the limit.
    pub fn finish(self) -> Result<CompleteScript, ActivityError> {
        if self.aborted {
            return Err(ActivityError::StreamAborted);
        }
        Ok(CompleteScript {
            request: self.request,
            environment: self.environment,
            limits: self.limits,
            python_runtime_profile: PythonRuntimeProfile::Default,
        })
    }
}

impl CompleteScript {
    /// Attaches explicit host runtime provenance for direct Python analysis.
    ///
    /// This does not verify the assertion or establish execution authority.
    /// Non-Python input with a non-default profile falls back in its entirety.
    pub const fn with_python_runtime_profile(mut self, profile: PythonRuntimeProfile) -> Self {
        self.python_runtime_profile = profile;
        self
    }

    /// Takes an already complete request, including optional host snapshots.
    ///
    /// This checks retained source length; use [`ScriptBuffer`] to bound incoming
    /// fragments before constructing a complete string. Neither bounds transport
    /// decoding or caller-owned memory. Snapshot comparison has separate limits.
    ///
    /// # Errors
    /// Returns [`ActivityError::SourceLimit`] if decoded source exceeds the limit.
    pub fn from_request(
        request: Request,
        environment: EnvironmentAssumptions,
        limits: AnalysisLimits,
    ) -> Result<Self, ActivityError> {
        if request.source.len() > limits.source_bytes() {
            return Err(ActivityError::SourceLimit);
        }
        Ok(Self {
            request,
            environment,
            limits,
            python_runtime_profile: PythonRuntimeProfile::Default,
        })
    }
}
