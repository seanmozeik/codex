//! Opt-in complete-command bridge; unwired to Codex dispatch and rendering.

use crate::AnalysisLimits;
use crate::Analyzer;
use crate::CompleteScript;
use crate::EnvironmentAssumptions;
use crate::GUARD_MAX_CALL_ID_BYTES;
use crate::GUARD_MAX_SOURCE_BYTES;
use crate::GuardedSourceActivity;
use crate::Language;
use crate::Request;
use crate::SourceAnalysisGuard;
use crate::UiRow;
use crate::WholeScriptActivity;
use codex_protocol::items::CommandExecutionItem;
use codex_protocol::protocol::ExecCommandSource;
use codex_shell_command::parse_command::extract_shell_command;
use serde::Serialize;
use std::ffi::OsStr;
use std::path::Path;
use std::time::Duration;

/// Original command item and separate, strictly filtered source presentation.
///
/// Fields are private so the borrowed item cannot be replaced independently of
/// its analysis. Serialization retains the complete original item alongside the
/// strict result. The host still applies its ordinary disclosure policy.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct CodexWholeScriptActivity<'a> {
    item: &'a CommandExecutionItem,
    source_activity: CodexWholeScriptSource<'a>,
}

/// Whole-source analysis or abstention at the actual command-item boundary.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
#[non_exhaustive]
pub enum CodexWholeScriptSource<'a> {
    /// Complete Bash source has been analyzed; its disposition may be fallback.
    Analyzed {
        /// Original shell executable spelling, without executable identity proof.
        shell: &'a str,
        /// Strict presentation; fallback contains no operations or diffs.
        activity: WholeScriptActivity,
    },
    /// Complete Bash source submitted through the shared, bounded-admission worker.
    Guarded {
        /// Original shell spelling, without executable identity proof.
        shell: &'a str,
        /// Guard and semantic fallback both retain zero additional rows/diffs.
        activity: GuardedSourceActivity,
    },
    /// No source analysis was attempted for this command-item shape.
    Unsupported {
        /// Carrier, lifecycle or source-size reason for preserving ordinary UI.
        reason: UnsupportedCodexWholeScript,
    },
}

/// Reasons the strict bridge retains only the ordinary command presentation.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum UnsupportedCodexWholeScript {
    /// Stdin interaction with a running process is not a complete initial script.
    Interaction,
    /// The existing Codex extractor did not identify a complete shell wrapper.
    UnrecognizedArgv,
    /// The extracted carrier is not Bash; other shell semantics are not asserted.
    UnsupportedShell,
    /// Decoded source exceeds the configured limit; it was not copied or parsed.
    SourceTooLarge,
    /// Call identity exceeds the worker cap; it was not copied or submitted.
    IdentityTooLarge,
    /// The complete-input boundary rejected a request after carrier validation.
    SourceRejected,
}

impl<'a> CodexWholeScriptActivity<'a> {
    /// Exact original item, including existing parsed commands and runtime state.
    #[must_use]
    pub const fn item(&self) -> &'a CommandExecutionItem {
        self.item
    }

    /// Final strict result or explicit boundary abstention.
    #[must_use]
    pub const fn source_activity(&self) -> &CodexWholeScriptSource<'a> {
        &self.source_activity
    }

    /// Additional static-intent rows only; never substitutes existing item rows.
    /// Every unsupported carrier and whole-script fallback yields an empty list.
    #[must_use]
    pub fn ui_rows(&self) -> Vec<UiRow<'_>> {
        match &self.source_activity {
            CodexWholeScriptSource::Analyzed { activity, .. } => activity.ui_rows(),
            CodexWholeScriptSource::Guarded { activity, .. } => activity.ui_rows(),
            CodexWholeScriptSource::Unsupported { .. } => Vec::new(),
        }
    }
}

/// Analyzes an actual command item's final, complete Bash source with strict gating.
///
/// Call only after transport assembly and pre-tool hook rewrites have finalized
/// the exact argv represented by `item`. `InProgress` describes process status,
/// not source completeness. Stdin interactions explicitly abstain even when the
/// item carries the original process argv. Generic `functions.exec` orchestration
/// is not a command item: only its actual child command executions belong here.
///
/// Existing shell extraction accepts exactly a shell, `-c`/`-lc`, and complete
/// script. This strict bridge further restricts the carrier basename to `bash`;
/// Sh, Zsh, PowerShell and direct interpreter argv retain ordinary presentation.
/// The host must establish [`EnvironmentAssumptions`], including ordinary tool
/// identity and absent startup effects; the executable name cannot prove either.
///
/// The original item is borrowed unchanged. Its executor cwd URI is retained;
/// the analyzer receives unresolved cwd rather than a host-local conversion.
/// No snapshots are supplied, files observed, code executed, status rewritten,
/// or sandbox/Guardian/approval path invoked or replaced. No renderer or dispatch
/// integration is installed by calling this prototype. The historical
/// [`crate::analyze_codex_command`] and contract remain independent baselines.
///
/// Generic orchestration source is not accepted as a command item:
///
/// ```compile_fail
/// use codex_code_activity::{AnalysisLimits, Analyzer, EnvironmentAssumptions,
///     analyze_complete_codex_command};
/// fn orchestration(analyzer: &mut Analyzer, javascript: &str) {
///     analyze_complete_codex_command(analyzer, javascript,
///         EnvironmentAssumptions::PersistentOrUnknown, AnalysisLimits::default());
/// }
/// ```
#[must_use]
pub fn analyze_complete_codex_command<'a>(
    analyzer: &mut Analyzer,
    item: &'a CommandExecutionItem,
    environment: EnvironmentAssumptions,
    limits: AnalysisLimits,
) -> CodexWholeScriptActivity<'a> {
    let source_activity = match complete_source(item, environment, limits) {
        Ok((shell, input)) => CodexWholeScriptSource::Analyzed {
            shell,
            activity: analyzer.analyze_complete(input),
        },
        Err(reason) => CodexWholeScriptSource::Unsupported { reason },
    };
    CodexWholeScriptActivity {
        item,
        source_activity,
    }
}

/// Submits a final actual command item through the shared source-analysis guard.
///
/// The carrier, completeness and runtime-assumption requirements of
/// [`analyze_complete_codex_command`] apply unchanged. Guard source and call-ID
/// byte caps are checked before copying borrowed item fields. No cwd or snapshots
/// are submitted, and the exact original item remains borrowed on every outcome.
///
/// This synchronous opt-in prototype waits at most the guard's capped response
/// budget, subject to host scheduling/cleanup. It does not stop worker CPU work
/// on timeout, execute submitted source, establish runtime assumptions, or wire
/// dispatch/rendering. See [`SourceAnalysisGuard`] for admission and failure limits.
#[must_use]
pub fn analyze_guarded_codex_command(
    item: &CommandExecutionItem,
    environment: EnvironmentAssumptions,
    limits: AnalysisLimits,
    wait: Duration,
) -> CodexWholeScriptActivity<'_> {
    let input = if item.id.as_str().len() > GUARD_MAX_CALL_ID_BYTES {
        Err(UnsupportedCodexWholeScript::IdentityTooLarge)
    } else {
        let limits = AnalysisLimits::new(
            limits.source_bytes().min(GUARD_MAX_SOURCE_BYTES),
            limits.elapsed(),
        );
        complete_source(item, environment, limits)
    };
    let source_activity = match input {
        Ok((shell, input)) => CodexWholeScriptSource::Guarded {
            shell,
            activity: SourceAnalysisGuard::shared().analyze(input, wait),
        },
        Err(reason) => CodexWholeScriptSource::Unsupported { reason },
    };
    CodexWholeScriptActivity {
        item,
        source_activity,
    }
}

fn complete_source(
    item: &CommandExecutionItem,
    environment: EnvironmentAssumptions,
    limits: AnalysisLimits,
) -> Result<(&str, CompleteScript), UnsupportedCodexWholeScript> {
    if item.source == ExecCommandSource::UnifiedExecInteraction || item.interaction_input.is_some()
    {
        return Err(UnsupportedCodexWholeScript::Interaction);
    }
    let Some((shell, source)) = extract_shell_command(&item.command) else {
        return Err(UnsupportedCodexWholeScript::UnrecognizedArgv);
    };
    if Path::new(shell).file_name() != Some(OsStr::new("bash")) {
        return Err(UnsupportedCodexWholeScript::UnsupportedShell);
    }
    if source.len() > limits.source_bytes() {
        return Err(UnsupportedCodexWholeScript::SourceTooLarge);
    }
    let input = CompleteScript::from_request(
        Request::new(
            item.id.as_str().into(),
            Language::Shell,
            source.into(),
            None,
        ),
        environment,
        limits,
    );
    match input {
        Ok(input) => Ok((shell, input)),
        Err(_) => Err(UnsupportedCodexWholeScript::SourceRejected),
    }
}
