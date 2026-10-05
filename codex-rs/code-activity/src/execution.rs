//! Terminal command-result decoding and bounded output.

use crate::ActivityError;
use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase")]
/// Terminal status supplied by the execution host.
#[non_exhaustive]
pub enum ExecutionStatus {
    /// The host observed a terminal process exit.
    Exited {
        /// Terminal process exit code.
        code: i32,
    },
    /// The host could not start the process.
    FailedToStart,
    /// The host cancelled execution.
    Cancelled,
    /// Evidence is insufficient for a more specific classification.
    Unknown,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
/// A terminal status and bounded UTF-8 output excerpt.
#[non_exhaustive]
pub struct ExecutionResult {
    /// Typed outcome supplied by the execution host.
    pub status: ExecutionStatus,
    /// At most 8 KiB of UTF-8 tool output.
    output: String,
    /// Whether the original output exceeded the excerpt limit.
    output_truncated: bool,
}

impl ExecutionResult {
    /// Bounds the output excerpt while preserving UTF-8 characters.
    #[must_use]
    pub fn new(status: ExecutionStatus, output: &str) -> Self {
        let mut end = output.len().min(8192);
        while !output.is_char_boundary(end) {
            end -= 1;
        }
        Self {
            status,
            output: output[..end].into(),
            output_truncated: end < output.len(),
        }
    }

    /// Adapter for the terminal `exec_command` result shape, not arbitrary stdout.
    /// A running session or missing exit code is not a completed execution.
    ///
    /// # Errors
    /// Returns an error for malformed result JSON, a running session or a missing exit code.
    pub fn from_exec_command_json(json: &str) -> Result<Self, ActivityError> {
        #[derive(Deserialize)]
        struct ResultFields {
            exit_code: Option<i32>,
            session_id: Option<serde_json::Value>,
            #[serde(default)]
            output: String,
        }
        let result: ResultFields = serde_json::from_str(json)?;
        if result.session_id.is_some() {
            return Err(ActivityError::SessionRunning);
        }
        let code = result.exit_code.ok_or(ActivityError::MissingExitCode)?;
        Ok(Self::new(ExecutionStatus::Exited { code }, &result.output))
    }
}

impl ExecutionResult {
    /// Borrows the bounded UTF-8 tool output excerpt.
    #[must_use]
    pub fn output(&self) -> &str {
        &self.output
    }
    /// Reports whether the original tool output exceeded the excerpt limit.
    #[must_use]
    pub const fn output_truncated(&self) -> bool {
        self.output_truncated
    }
}
