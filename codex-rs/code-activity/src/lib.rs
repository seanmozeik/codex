//! Interpret agent-generated code as provisional activity, then compare host snapshots.
//!
//! Source analysis never executes code. File observation is an explicit host action.
//!
//! # Examples
//!
//! ```
//! use codex_code_activity::{Analyzer, Language, Request};
//! let mut analyzer = Analyzer::new()?;
//! let request = Request::new("demo".into(), Language::Python, "open('a').read()".into(), None);
//! let report = analyzer.analyze(request);
//! assert_eq!(report.operations.len(), 1);
//! # Ok::<(), codex_code_activity::ActivityError>(())
//! ```

#![forbid(unsafe_code)]
#![deny(missing_docs, missing_debug_implementations, unused_must_use)]

mod capture_error;
mod codex_adapter;
mod codex_whole_script;
pub mod contract;
mod diff;
mod error;
mod execution;
mod executor_adapter;
mod identity;
mod interpret;
mod lifecycle;
mod model;
mod orchestration;
mod phase;
pub mod policy;
mod python_profile;
mod script_buffer;
mod selective_edits;
mod shell;
mod snapshot;
mod source_description;
mod source_guard;
mod source_guard_result;
mod source_tree;
mod stream;
mod strict_syntax;
mod trusted_pathlib;
mod value;
mod whole_script;

pub use capture_error::CaptureError;
pub use codex_adapter::CodexCommandActivity;
pub use codex_adapter::CodexSourceActivity;
pub use codex_adapter::UnsupportedCodexCarrier;
pub use codex_adapter::analyze_codex_command;
pub use codex_whole_script::CodexWholeScriptActivity;
pub use codex_whole_script::CodexWholeScriptSource;
pub use codex_whole_script::UnsupportedCodexWholeScript;
pub use codex_whole_script::analyze_complete_codex_command;
pub use codex_whole_script::analyze_guarded_codex_command;
pub use contract::ActivityContract;
pub use contract::ArgumentKnowledge;
pub use contract::ContractAnalysis;
pub use contract::ContractSchema;
pub use contract::UiAction;
pub use contract::UiRow;
pub use error::ActivityError;
pub use execution::ExecutionResult;
pub use execution::ExecutionStatus;
pub use executor_adapter::ExecutorContext;
pub use executor_adapter::ExecutorDispatch;
pub use executor_adapter::ExecutorPresentation;
pub use executor_adapter::ExecutorRuntimeProfile;
pub use executor_adapter::ExecutorSource;
pub use executor_adapter::FinalExecutorInput;
pub use executor_adapter::PreparedExecutor;
pub use identity::CallId;
pub use identity::OperationId;
pub use identity::Revision;
pub use identity::SourceId;
pub use lifecycle::CompletedActivity;
pub use lifecycle::ObservedChange;
pub use lifecycle::ObservedFile;
pub use lifecycle::PreparedActivity;
pub use lifecycle::PreparedCall;
pub use lifecycle::PreparedFile;
pub use model::Basis;
pub use model::Coverage;
pub use model::DiffAttribution;
pub use model::Effect;
pub use model::FileDiff;
pub use model::FileSnapshot;
pub use model::Language;
pub use model::Operation;
pub use model::Report;
pub use model::Request;
pub use model::Source;
pub use model::Span;
pub use model::Target;
pub use model::Unresolved;
pub use model::WriteMode;
pub use orchestration::CompleteOrchestration;
pub use orchestration::OrchestrationActivity;
pub use orchestration::OrchestrationBuffer;
pub use orchestration::OrchestrationChild;
pub use orchestration::OrchestrationChildId;
pub use orchestration::OrchestrationDisposition;
pub use orchestration::OrchestrationFallback;
pub use orchestration::OrchestrationGroup;
pub use orchestration::OrchestrationInputError;
pub use orchestration::OrchestrationJoin;
pub use orchestration::OrchestratorBindings;
pub use phase::CompletedPhase;
pub use phase::ObservationBasis;
pub use phase::PendingPhase;
pub use phase::PreparedPhase;
pub use phase::ReportVersion;
pub use phase::Transform;
pub use python_profile::PythonRuntimeProfile;
pub use script_buffer::CompleteScript;
pub use script_buffer::ScriptBuffer;
pub use selective_edits::*;
pub use snapshot::FileState;
pub use snapshot::LocalFiles;
pub use snapshot::MAX_CAPTURE_BYTES;
pub use snapshot::MAX_CAPTURE_FILES;
pub use snapshot::MAX_FILE_BYTES;
pub use snapshot::SnapshotProvider;
pub use snapshot::SnapshotSummary;
pub use source_description::CompleteSourceDescriptionInput;
pub use source_description::LiteralPatchPath;
pub use source_description::PatchByteRange;
pub use source_description::PatchChunk;
pub use source_description::PatchEvidence;
pub use source_description::PatchId;
pub use source_description::PatchLine;
pub use source_description::PatchLinePrefix;
pub use source_description::PatchPathReference;
pub use source_description::SourceDescription;
pub use source_description::SourceDescriptionDisposition;
pub use source_description::SourceDescriptionFallback;
pub use source_description::SourcePatch;
pub use source_description::SourcePatchFile;
pub use source_description::SourceReference;
pub use source_description::SourceReferenceBasis;
pub use source_description::SourceReferenceKind;
pub use source_guard::GUARD_MAX_CALL_ID_BYTES;
pub use source_guard::GUARD_MAX_CWD_BYTES;
pub use source_guard::GUARD_MAX_SOURCE_BYTES;
pub use source_guard::GUARD_MAX_WAIT;
pub use source_guard::GuardedSourcePresentation;
pub use source_guard::SourceAnalysisGuard;
pub use source_guard::SourcePresentationInput;
pub use source_guard::SourcePresentationMode;
pub use source_guard_result::GuardDisposition;
pub use source_guard_result::GuardFallback;
pub use source_guard_result::GuardedOrchestrationActivity;
pub use source_guard_result::GuardedSourceActivity;
pub use source_guard_result::GuardedSourceDescription;
pub use source_guard_result::SourceDescriptionGuardDisposition;
pub(crate) use source_tree::children;
pub(crate) use source_tree::span;
pub(crate) use source_tree::text;
pub use stream::ActivityStream;
pub use stream::FinalCall;
pub use stream::PendingActivity;
use tree_sitter::Parser;
pub use whole_script::AnalysisLimits;
pub use whole_script::EnvironmentAssumptions;
pub use whole_script::FallbackReason;
pub use whole_script::PresentationDisposition;
pub use whole_script::WholeScriptActivity;

/// Maximum bytes accepted in one decoded source.
pub const MAX_SOURCE_BYTES: usize = 1_048_576;
/// Maximum reported operations and unresolved regions per report.
pub const MAX_OPERATIONS: usize = 256;
/// Maximum recursive syntax traversal depth.
pub const MAX_NESTING: usize = 64;

/// Reuse one analyser per worker to amortise parser setup. No input code is executed.
pub struct Analyzer {
    python: Parser,
    typescript: Parser,
    shell: Parser,
    preview_shell: Parser,
    budget: Option<whole_script::AnalysisBudget>,
}

impl std::fmt::Debug for Analyzer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Analyzer").finish_non_exhaustive()
    }
}

impl Analyzer {
    /// Configures the three supported source grammars.
    ///
    /// # Errors
    /// Returns an error if a grammar is incompatible with the parser runtime.
    pub fn new() -> Result<Self, ActivityError> {
        fn parser(language: &tree_sitter::Language) -> Result<Parser, ActivityError> {
            let mut p = Parser::new();
            p.set_language(language)?;
            Ok(p)
        }
        Ok(Self {
            python: parser(&tree_sitter_python::LANGUAGE.into())?,
            typescript: parser(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())?,
            shell: parser(&tree_sitter_bash::LANGUAGE.into())?,
            preview_shell: parser(&codex_tree_sitter_bash::LANGUAGE.into())?,
            budget: None,
        })
    }
    /// Interprets a complete cell and compares any supplied text snapshots.
    #[must_use]
    pub fn analyze(&mut self, request: Request) -> Report {
        let mut report = Report {
            version: ReportVersion::V1,
            call_id: request.call_id,
            coverage: Coverage::Partial,
            sources: vec![],
            operations: vec![],
            unresolved: vec![],
            diffs: vec![],
        };
        self.source(
            &request.source,
            request.language,
            request.cwd,
            /*parent*/ None,
            /*depth*/ 0,
            &mut report,
        );
        report.diffs = diff::compare(request.snapshots);
        if report.operations.is_empty() {
            report.coverage = Coverage::Opaque;
        }
        report
    }
    pub(crate) fn source(
        &mut self,
        source: &str,
        language: Language,
        cwd: Option<String>,
        parent: Option<Span>,
        depth: usize,
        report: &mut Report,
    ) {
        if report.unresolved.len() >= MAX_OPERATIONS {
            return;
        }
        if report.sources.len() >= 64 {
            if let Some(evidence) = parent
                && report.unresolved.len() < MAX_OPERATIONS
            {
                report.unresolved.push(Unresolved {
                    evidence,
                    reason: "Embedded source budget exceeded".into(),
                });
            }
            return;
        }
        let id = SourceId(report.sources.len());
        report.sources.push(Source {
            id,
            language,
            parent,
        });
        let root_span = Span {
            source_id: id,
            start_byte: 0,
            end_byte: source.len(),
            start_line: 1,
            end_line: 1,
            start_column: 1,
        };
        if source.len() > MAX_SOURCE_BYTES || depth > 4 {
            report.unresolved.push(Unresolved {
                evidence: root_span,
                reason: "Source or nesting budget exceeded".into(),
            });
            return;
        }
        if self
            .budget
            .is_some_and(whole_script::AnalysisBudget::expired)
        {
            report.unresolved.push(Unresolved {
                evidence: root_span,
                reason: "Whole-script analysis time budget exceeded".into(),
            });
            return;
        }
        let parser = match language {
            Language::Python => &mut self.python,
            Language::TypeScript => &mut self.typescript,
            Language::Shell => &mut self.shell,
        };
        let tree = whole_script::parse(parser, source, self.budget);
        let Some(tree) = tree else {
            report.unresolved.push(Unresolved {
                evidence: root_span,
                reason: ActivityError::ParseInterrupted.to_string(),
            });
            return;
        };
        if tree.root_node().has_error() {
            report.unresolved.push(Unresolved {
                evidence: root_span,
                reason: "Incomplete or invalid syntax; no effects inferred".into(),
            });
            return;
        }
        if let Some(budget) = self.budget
            && let Some(gap) = strict_syntax::check(tree.root_node(), source, id, language, budget)
        {
            report.unresolved.push(gap);
            return;
        }
        if language == Language::Shell {
            shell::analyze(self, tree.root_node(), source, id, cwd, depth, report);
        } else {
            let mode = if self.budget.is_some() {
                interpret::Mode::Complete
            } else {
                interpret::Mode::Diagnostic
            };
            let embedded =
                interpret::analyze(tree.root_node(), source, id, language, cwd, report, mode);
            for e in embedded {
                self.source(
                    &e.source,
                    e.language,
                    e.cwd,
                    Some(e.parent),
                    depth + 1,
                    report,
                );
            }
        }
    }
}
