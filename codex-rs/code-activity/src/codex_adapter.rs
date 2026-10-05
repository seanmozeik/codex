//! Unwired prototype adapter for Codex's actual core command item.
//!
//! This bridge leaves command parsing, dispatch, authorization and process status
//! untouched. Its serialization is a local demonstration, not an app-server API;
//! raw items and reports require host disclosure policy before client delivery.

use crate::Analyzer;
use crate::Language;
use crate::MAX_SOURCE_BYTES;
use crate::Report;
use crate::Request;
use codex_protocol::items::CommandExecutionItem;
use codex_shell_command::bash::extract_bash_command;
use codex_shell_command::parse_command::extract_shell_command;
use serde::Serialize;

/// A borrowed Codex command item alongside independent static source analysis.
///
/// The original item preserves argv, parsed commands, executor cwd URI,
/// process ID and status.
/// The adapter never updates that item, observes files or produces file receipts.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct CodexCommandActivity<'a> {
    /// The original core item; its lifecycle is independent of static intent.
    pub item: &'a CommandExecutionItem,
    /// Supported static analysis or an explicit carrier/budget abstention.
    pub source_activity: CodexSourceActivity<'a>,
}

/// Static intent for a supported source carrier, or the reason for abstention.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
#[non_exhaustive]
pub enum CodexSourceActivity<'a> {
    /// Bash-grammar analysis of original Sh/Bash/Zsh command source.
    Supported {
        /// Shell executable spelling preserved from the original argv.
        shell: &'a str,
        /// Partial or opaque static intent; no snapshots or observed diffs.
        report: Report,
    },
    /// A carrier or source size outside this prototype's supported scope.
    Unsupported {
        /// Why no report was generated; this is not a claim of no file effects.
        reason: UnsupportedCodexCarrier,
    },
}

/// Reasons why this bridge abstains before source analysis.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum UnsupportedCodexCarrier {
    /// Codex's extractor did not recognize a shell source wrapper.
    UnrecognizedArgv,
    /// Codex recognized a shell whose syntax is not supported by this parser.
    UnsupportedShell,
    /// The decoded source exceeds the parser's byte budget.
    SourceTooLarge,
}

/// Analyses original command source while preserving the actual core item.
///
/// Only wrappers accepted by Codex's shell extractor and Bash carrier extractor
/// are supported. PowerShell and direct Python/Node argv explicitly abstain.
/// Zsh coverage is best-effort Bash-grammar analysis with unresolved regions.
/// Source remains static intent even for a completed or failed command item.
///
/// Executor cwd remains on `item`; analysis starts with unresolved cwd while
/// retaining literal target spelling. Source may include conditional cwd changes. A production host must resolve them using executor-native path
/// semantics. Passing a URI through the parser's string cwd would make `cd`
/// concatenate paths incorrectly, especially for foreign Windows executors.
/// No source is executed and no filesystem is accessed. Call this after any
/// pre-tool hook rewrite when analysing the source that will actually dispatch.
#[must_use]
pub fn analyze_codex_command<'a>(
    analyzer: &mut Analyzer,
    item: &'a CommandExecutionItem,
) -> CodexCommandActivity<'a> {
    let source_activity = match extract_shell_command(&item.command) {
        None => CodexSourceActivity::Unsupported {
            reason: UnsupportedCodexCarrier::UnrecognizedArgv,
        },
        Some(_) if extract_bash_command(&item.command).is_none() => {
            CodexSourceActivity::Unsupported {
                reason: UnsupportedCodexCarrier::UnsupportedShell,
            }
        }
        Some((_, source)) if source.len() > MAX_SOURCE_BYTES => CodexSourceActivity::Unsupported {
            reason: UnsupportedCodexCarrier::SourceTooLarge,
        },
        Some((shell, source)) => CodexSourceActivity::Supported {
            shell,
            report: analyzer.analyze(Request::new(
                item.id.as_str().into(),
                Language::Shell,
                source.into(),
                /*cwd*/ None,
            )),
        },
    };
    CodexCommandActivity {
        item,
        source_activity,
    }
}
