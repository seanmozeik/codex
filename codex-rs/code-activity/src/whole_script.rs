//! All-or-nothing static presentation; never execution or permission authority.

use crate::ActivityContract;
use crate::Analyzer;
use crate::ArgumentKnowledge;
use crate::CompleteScript;
use crate::Coverage;
use crate::Effect;
use crate::FileDiff;
use crate::MAX_SOURCE_BYTES;
use crate::PythonRuntimeProfile;
use crate::Report;
use crate::ReportVersion;
use crate::UiRow;
use crate::WriteMode;
use serde::Serialize;
use std::time::Duration;
use std::time::Instant;
use tree_sitter::ParseOptions;
use tree_sitter::Parser;
use tree_sitter::Tree;

/// Host assertions required before interpreting a fresh lexical environment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum EnvironmentAssumptions {
    /// Fresh globals and ordinary unmodified builtins/module resolution, without
    /// startup hooks, injected bindings, shell aliases/functions or prior cells.
    /// Recognized command and carrier names must resolve to their ordinary tools,
    /// including PATH lookup and explicit relative/absolute executable paths.
    /// Fresh process creation alone does not establish these identities.
    /// Initial strict JavaScript use assumes Node-style runtime semantics;
    /// Deno runtime references are unsupported and always cause fallback.
    /// This is a host assertion, not something source parsing can verify. Imports
    /// still retain the baseline's module-initialization uncertainty and fall back.
    FreshProcess,
    /// Persistent REPL state, modified runtime, or assumptions not established.
    PersistentOrUnknown,
}

/// Source storage limit and a cooperative elapsed-time budget.
///
/// The byte ceiling bounds retained decoded source length, not total process
/// memory. Time is checked at source boundaries, tree-sitter progress callbacks,
/// and after interpretation/comparison. It is **not a hard wall-clock bound**:
/// callbacks are periodic, and semantic traversal/diff work cannot be preempted.
/// A host needing a hard deadline must isolate work in a cancellable process.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct AnalysisLimits {
    source_bytes: usize,
    elapsed: Duration,
}

impl AnalysisLimits {
    /// Configures limits, clamping source bytes to [`MAX_SOURCE_BYTES`].
    /// Zero elapsed time deliberately disables enrichment.
    #[must_use]
    pub fn new(source_bytes: usize, elapsed: Duration) -> Self {
        Self {
            source_bytes: source_bytes.min(MAX_SOURCE_BYTES),
            elapsed,
        }
    }

    /// Maximum retained decoded bytes.
    #[must_use]
    pub const fn source_bytes(self) -> usize {
        self.source_bytes
    }

    /// Cooperative budget shared by the outer and every nested source.
    #[must_use]
    pub const fn elapsed(self) -> Duration {
        self.elapsed
    }
}

impl Default for AnalysisLimits {
    fn default() -> Self {
        Self::new(MAX_SOURCE_BYTES, Duration::from_millis(100))
    }
}

/// Why the complete source uses the host's ordinary raw presentation.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum FallbackReason {
    /// Fresh, ordinary runtime bindings were not asserted by the host.
    EnvironmentUnknown,
    /// Cooperative elapsed-time check expired, possibly after work completed.
    TimeBudget,
    /// Any syntax, semantic, nested-source or resource-budget gap was reported.
    UnresolvedSource,
    /// A recognized operation had unknown arguments or an unknown write mode.
    UnknownArguments,
    /// Child process effects are not established by literal launch arguments.
    ProcessEffectsUnknown,
    /// A host snapshot comparison exceeded its file-count or byte budget.
    SnapshotBudget,
}

/// Whole-source presentation eligibility, never an authorization decision.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "type", content = "reason", rename_all = "camelCase")]
#[non_exhaustive]
pub enum PresentationDisposition {
    /// All recognized static intent is eligible under the stated assumptions.
    /// An empty report is allowed; this is not proof of an effect-free program.
    Eligible,
    /// All rich rows and diffs were discarded; retain normal raw UI and controls.
    Fallback(FallbackReason),
}

/// Final source presentation with no mutable or unfiltered report escape hatch.
///
/// Fallback reports retain diagnostics but contain zero operations and diffs.
/// This type never changes execution, sandbox, Guardian or approval decisions.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WholeScriptActivity {
    disposition: PresentationDisposition,
    report: Report,
}

impl WholeScriptActivity {
    /// Whole-source eligibility or explicit fallback reason.
    #[must_use]
    pub const fn disposition(&self) -> PresentationDisposition {
        self.disposition
    }

    /// Final filtered report. No recognized prefix survives fallback.
    #[must_use]
    pub const fn report(&self) -> &Report {
        &self.report
    }

    /// Static intent rows; empty for every fallback.
    #[must_use]
    pub fn ui_rows(&self) -> Vec<UiRow<'_>> {
        ActivityContract::from_report(&self.report).ui_rows()
    }

    /// Host endpoint comparisons; empty for every fallback.
    /// These remain net observations, not proof any source operation executed.
    #[must_use]
    pub fn diffs(&self) -> &[FileDiff] {
        &self.report.diffs
    }
}

impl Analyzer {
    /// Analyzes only complete buffered input and atomically gates presentation.
    ///
    /// Every unresolved region, unknown target/mode, opaque child process,
    /// unsatisfied runtime assumption, or expired budget discards **all** rows
    /// and diffs. Default profiles retain every import gap; the explicit preloaded
    /// pathlib profile uses a separate bounded interpreter. Existing [`Self::analyze`]
    /// remains the partial diagnostic baseline. Strict-only syntax guards also
    /// reject Python assertions, comparisons and version-sensitive annotations
    /// and JavaScript complex assignment/update targets whose evaluation is not
    /// fully modeled by that baseline. Python calls with more than two positional
    /// arguments also fall back, including local functions: this conservative
    /// syntactic limit catches positional option/callback effects through aliases.
    /// Deno identifiers also cause whole-script fallback, including alias bindings;
    /// Deno creation and permission options are outside the initial strict scope.
    /// The host must retain the exact original command and continue its normal
    /// raw presentation, dispatch and authorization path on fallback or errors.
    ///
    /// # Examples
    /// ```
    /// use codex_code_activity::{AnalysisLimits, Analyzer, CompleteScript,
    ///     EnvironmentAssumptions, Language, PresentationDisposition, Request};
    /// let input = CompleteScript::from_request(
    ///     Request::new("example".into(), Language::Python, "open('a').read()".into(), None),
    ///     EnvironmentAssumptions::FreshProcess, AnalysisLimits::default())?;
    /// let activity = Analyzer::new()?.analyze_complete(input);
    /// assert_eq!(activity.disposition(), PresentationDisposition::Eligible);
    /// assert_eq!(activity.ui_rows().len(), 1);
    /// # Ok::<(), codex_code_activity::ActivityError>(())
    /// ```
    #[must_use]
    pub fn analyze_complete(&mut self, script: CompleteScript) -> WholeScriptActivity {
        let CompleteScript {
            mut request,
            environment,
            limits,
            python_runtime_profile,
        } = script;
        if environment != EnvironmentAssumptions::FreshProcess
            || (python_runtime_profile != PythonRuntimeProfile::Default
                && request.language != crate::Language::Python)
        {
            return WholeScriptActivity {
                disposition: PresentationDisposition::Fallback(FallbackReason::EnvironmentUnknown),
                report: Report {
                    version: ReportVersion::V1,
                    call_id: request.call_id,
                    coverage: Coverage::Opaque,
                    sources: vec![],
                    operations: vec![],
                    unresolved: vec![],
                    diffs: vec![],
                },
            };
        }
        let budget = AnalysisBudget {
            started: Instant::now(),
            limit: limits.elapsed,
        };
        let snapshots = std::mem::take(&mut request.snapshots);
        self.budget = Some(budget);
        let mut report = match python_runtime_profile {
            PythonRuntimeProfile::Default => self.analyze(request),
            PythonRuntimeProfile::PreloadedTrustedPathlib => {
                crate::trusted_pathlib::analyze(&mut self.python, request, budget)
            }
        };
        self.budget = None;
        let mut reason = fallback_reason(&report, budget);
        if reason.is_none() {
            report.diffs = crate::diff::compare(snapshots);
            reason = fallback_reason(&report, budget);
        }
        let disposition = match reason {
            Some(reason) => {
                report.operations.clear();
                report.diffs.clear();
                report.coverage = Coverage::Opaque;
                PresentationDisposition::Fallback(reason)
            }
            None => PresentationDisposition::Eligible,
        };
        WholeScriptActivity {
            disposition,
            report,
        }
    }
}

fn fallback_reason(report: &Report, budget: AnalysisBudget) -> Option<FallbackReason> {
    if budget.expired() {
        return Some(FallbackReason::TimeBudget);
    }
    if !report.unresolved.is_empty() {
        return Some(FallbackReason::UnresolvedSource);
    }
    if report
        .diffs
        .iter()
        .any(|diff| matches!(diff, FileDiff::Omitted { .. } | FileDiff::Truncated { .. }))
    {
        return Some(FallbackReason::SnapshotBudget);
    }
    for operation in &report.operations {
        if crate::contract::argument_knowledge(&operation.effect) == ArgumentKnowledge::Unknown
            || matches!(
                operation.effect,
                Effect::FileOpen {
                    mode: WriteMode::Unknown,
                    ..
                } | Effect::FileWrite {
                    mode: WriteMode::Unknown,
                    ..
                }
            )
        {
            return Some(FallbackReason::UnknownArguments);
        }
        if matches!(operation.effect, Effect::ProcessRun { .. }) {
            return Some(FallbackReason::ProcessEffectsUnknown);
        }
    }
    None
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AnalysisBudget {
    started: Instant,
    limit: Duration,
}

impl AnalysisBudget {
    pub(crate) fn expired(self) -> bool {
        self.started.elapsed() >= self.limit
    }
}

pub(crate) fn parse(
    parser: &mut Parser,
    source: &str,
    budget: Option<AnalysisBudget>,
) -> Option<Tree> {
    let Some(budget) = budget else {
        return parser.parse(source, None);
    };
    let bytes = source.as_bytes();
    let mut progress = |_: &tree_sitter::ParseState| budget.expired();
    let tree = parser.parse_with_options(
        &mut |offset, _| bytes.get(offset..).unwrap_or_default(),
        None,
        Some(ParseOptions::new().progress_callback(&mut progress)),
    );
    if tree.is_none() {
        // A cancelled tree-sitter parse retains continuation state unless reset.
        parser.reset();
    }
    tree
}

#[cfg(test)]
mod tests {
    use super::AnalysisBudget;
    use super::parse;
    use crate::ActivityError;
    use pretty_assertions::assert_eq;
    use std::time::Duration;
    use std::time::Instant;
    use tree_sitter::Parser;

    #[test]
    fn cancelled_parse_cannot_resume_in_the_next_input() -> Result<(), ActivityError> {
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_python::LANGUAGE.into())?;
        let source = "value = 1\n".repeat(20_000);
        // Exercise tree-sitter's real progress callback, bypassing source-boundary checks.
        let budget = AnalysisBudget {
            started: Instant::now(),
            limit: Duration::ZERO,
        };
        assert!(parse(&mut parser, &source, Some(budget)).is_none());
        let next = "open('fresh').read()";
        let reused = parse(&mut parser, next, None).ok_or(ActivityError::ParseInterrupted)?;
        let mut fresh = Parser::new();
        fresh.set_language(&tree_sitter_python::LANGUAGE.into())?;
        let expected = fresh
            .parse(next, None)
            .ok_or(ActivityError::ParseInterrupted)?;
        assert_eq!(reused.root_node().to_sexp(), expected.root_node().to_sexp());
        assert_eq!(reused.root_node().end_byte(), next.len());
        Ok(())
    }
}
