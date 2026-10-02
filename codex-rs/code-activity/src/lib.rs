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
pub mod contract;
mod diff;
mod error;
mod execution;
mod identity;
mod interpret;
mod lifecycle;
mod model;
mod phase;
pub mod policy;
mod shell;
mod snapshot;
mod stream;
mod value;

pub use capture_error::CaptureError;
pub use codex_adapter::CodexCommandActivity;
pub use codex_adapter::CodexSourceActivity;
pub use codex_adapter::UnsupportedCodexCarrier;
pub use codex_adapter::analyze_codex_command;
pub use contract::ActivityContract;
pub use contract::ArgumentKnowledge;
pub use contract::ContractAnalysis;
pub use contract::ContractSchema;
pub use contract::UiAction;
pub use contract::UiRow;
pub use error::ActivityError;
pub use execution::ExecutionResult;
pub use execution::ExecutionStatus;
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
pub use phase::CompletedPhase;
pub use phase::ObservationBasis;
pub use phase::PendingPhase;
pub use phase::PreparedPhase;
pub use phase::ReportVersion;
pub use phase::Transform;
pub use snapshot::FileState;
pub use snapshot::LocalFiles;
pub use snapshot::MAX_CAPTURE_BYTES;
pub use snapshot::MAX_CAPTURE_FILES;
pub use snapshot::MAX_FILE_BYTES;
pub use snapshot::SnapshotProvider;
pub use snapshot::SnapshotSummary;
pub use stream::ActivityStream;
pub use stream::FinalCall;
pub use stream::PendingActivity;
use tree_sitter::Node;
use tree_sitter::Parser;

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
            None,
            0,
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
        let parser = match language {
            Language::Python => &mut self.python,
            Language::TypeScript => &mut self.typescript,
            Language::Shell => &mut self.shell,
        };
        let Some(tree) = parser.parse(source, None) else {
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
        if language == Language::Shell {
            shell::analyze(self, tree.root_node(), source, id, cwd, depth, report);
        } else {
            let embedded = interpret::analyze(tree.root_node(), source, id, language, cwd, report);
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

pub(crate) fn span(node: Node<'_>, source_id: SourceId) -> Span {
    Span {
        source_id,
        start_byte: node.start_byte(),
        end_byte: node.end_byte(),
        start_line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
        start_column: node.start_position().column + 1,
    }
}
pub(crate) fn children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}
pub(crate) fn text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    &source[node.byte_range()]
}
