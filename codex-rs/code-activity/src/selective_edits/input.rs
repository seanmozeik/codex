//! Complete submitted source for conditional edit forecasts.

use crate::ActivityError;
use crate::AnalysisLimits;
use crate::CallId;
use crate::GUARD_MAX_CALL_ID_BYTES;
use crate::GUARD_MAX_CWD_BYTES;
use crate::GUARD_MAX_SOURCE_BYTES;
use crate::Language;
use serde::Deserialize;
use serde::Serialize;

/// Selects the endpoint of an owned Python text-write projection.
///
/// This choice precedes snapshot validation. It does not establish successful
/// execution, file identity or buffered-byte realization.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum PythonWriteEndpoint {
    /// Retain the write-call logical acceptance projection when source provides
    /// an explicit newline policy. Omitted policies keep the legacy close view.
    #[default]
    LogicalAcceptance,
    /// For a supported single-write `w` context, project bytes conditionally at
    /// close. Update-mode logical prefixes remain available: this view does not
    /// invent a physical realization for an unsupported `r+` continuation.
    ContextCloseWhenAvailable,
}

/// Complete input for dependency-aware, conditional UI forecasts.
///
/// Construction asserts source completeness only. It does not assert standard
/// runtime bindings, establish file identity, or authorize execution. Forecast
/// assumptions are returned explicitly; strict runtime-gated APIs are separate.
#[derive(Debug)]
#[must_use]
pub struct SelectiveEditInput {
    pub(crate) call_id: CallId,
    pub(crate) kind: SourceKind,
    pub(crate) source: String,
    pub(crate) cwd: Option<String>,
    pub(crate) limits: AnalysisLimits,
    pub(crate) python_write_endpoint: PythonWriteEndpoint,
}

/// Source semantics are distinct from the parser grammar. In particular, a
/// CodeMode cell and a Node CommonJS script both use the TypeScript grammar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SourceKind {
    Python,
    Shell,
    CodeMode,
    NodeCommonJs,
}

impl SourceKind {
    pub(crate) const fn from_language(language: Language) -> Self {
        match language {
            Language::Python => Self::Python,
            Language::Shell => Self::Shell,
            Language::TypeScript => Self::CodeMode,
        }
    }

    pub(crate) const fn language(self) -> Language {
        match self {
            Self::Python => Language::Python,
            Self::Shell => Language::Shell,
            Self::CodeMode | Self::NodeCommonJs => Language::TypeScript,
        }
    }
}

impl SelectiveEditInput {
    /// Copies bounded, complete source and an optional host-provided cwd anchor.
    /// The original command remains the host's execution input.
    ///
    /// # Errors
    /// Returns [`ActivityError::SourceLimit`] for an oversized input or anchor.
    pub fn from_source(
        call_id: CallId,
        language: Language,
        source: &str,
        cwd: Option<&str>,
        limits: AnalysisLimits,
    ) -> Result<Self, ActivityError> {
        if source.len() > limits.source_bytes().min(GUARD_MAX_SOURCE_BYTES)
            || call_id.as_str().len() > GUARD_MAX_CALL_ID_BYTES
            || cwd.is_some_and(|value| value.len() > GUARD_MAX_CWD_BYTES || value.contains('\0'))
        {
            return Err(ActivityError::SourceLimit);
        }
        Ok(Self {
            call_id,
            kind: SourceKind::from_language(language),
            source: source.to_owned(),
            cwd: cwd.map(str::to_owned),
            limits,
            python_write_endpoint: PythonWriteEndpoint::default(),
        })
    }

    /// Copies a complete script for the explicitly conditional Node CommonJS
    /// forecast lane. This declaration does not attest the runtime's identity,
    /// module cache, startup hooks, globals or standard-library behavior.
    /// Ordinary [`Self::from_source`] TypeScript inputs remain CodeMode cells.
    ///
    /// # Errors
    /// Returns [`ActivityError::SourceLimit`] for an oversized input or anchor.
    pub fn from_node_commonjs(
        call_id: CallId,
        source: &str,
        cwd: Option<&str>,
        limits: AnalysisLimits,
    ) -> Result<Self, ActivityError> {
        let mut input = Self::from_source(call_id, Language::TypeScript, source, cwd, limits)?;
        input.kind = SourceKind::NodeCommonJs;
        Ok(input)
    }

    /// Exact supplied source; captured programs are never executed by analysis.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Selects an owned Python write endpoint for this source and its children.
    /// The default is [`PythonWriteEndpoint::LogicalAcceptance`]. Selection does
    /// not depend on whether a later snapshot binding or preview is accepted.
    pub fn with_python_write_endpoint(mut self, endpoint: PythonWriteEndpoint) -> Self {
        self.python_write_endpoint = endpoint;
        self
    }

    /// Requested grammar, without an assertion about executable identity.
    #[must_use]
    pub const fn language(&self) -> Language {
        self.kind.language()
    }

    pub(crate) fn clamp_elapsed(&mut self, remaining: std::time::Duration) {
        self.limits = AnalysisLimits::new(
            self.limits.source_bytes(),
            self.limits.elapsed().min(remaining),
        );
    }
}
