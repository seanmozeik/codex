use crate::ActivityError;
use crate::Analyzer;
use crate::CallId;
use crate::Language;
use crate::MAX_SOURCE_BYTES;
use crate::PendingPhase;
use crate::Report;
use crate::Request;
use crate::Revision;
use serde::Serialize;
use tree_sitter::InputEdit;
use tree_sitter::Parser;
use tree_sitter::Point;
use tree_sitter::Tree;

/// A replacement view, not an append-only event log. IDs belong to this revision.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct PendingActivity {
    /// Provisional source phase; this cannot be changed to a completed marker.
    pub phase: PendingPhase,
    /// Counter used to discard stale replacement views.
    pub revision: Revision,
    /// Bytes received so far, including incomplete syntax.
    pub source_bytes: usize,
    /// Bytes included in semantic analysis of the contiguous valid prefix.
    pub analyzed_bytes: usize,
    /// Whether the received prefix currently parses without errors.
    pub syntax_complete: bool,
    /// All pending operations for this revision.
    pub report: Report,
}

/// Append decoded source fragments. No filesystem access occurs during streaming.
/// The host owns transport JSON decoding and stream cancellation.
///
/// Capture cannot start while the source is still streaming:
///
/// ```compile_fail
/// use codex_code_activity::{ActivityStream, SnapshotProvider};
/// fn capture_early(stream: ActivityStream, files: &mut impl SnapshotProvider) {
///     stream.prepare(files, &[]);
/// }
/// ```
pub struct ActivityStream {
    call_id: CallId,
    language: Language,
    cwd: Option<String>,
    source: String,
    revision: Revision,
    parser: Parser,
    tree: Option<Tree>,
    analyzer: Analyzer,
    failed: bool,
}

impl std::fmt::Debug for ActivityStream {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ActivityStream")
            .field("call_id", &self.call_id)
            .field("language", &self.language)
            .field("revision", &self.revision)
            .field("source_bytes", &self.source.len())
            .finish_non_exhaustive()
    }
}

/// A frozen analysis. Only this state can prepare filesystem observation.
#[derive(Debug)]
#[must_use]
pub struct FinalCall {
    pub(crate) report: Report,
}

impl FinalCall {
    /// Borrows the frozen source analysis before capture.
    #[must_use]
    pub const fn report(&self) -> &Report {
        &self.report
    }
}

impl ActivityStream {
    /// Starts a preview with fresh bindings and parsers.
    ///
    /// # Errors
    /// Returns an error if a grammar is incompatible with the parser runtime.
    pub fn new(
        call_id: CallId,
        language: Language,
        cwd: Option<String>,
    ) -> Result<Self, ActivityError> {
        let mut parser = Parser::new();
        let grammar = match language {
            Language::Python => tree_sitter_python::LANGUAGE.into(),
            Language::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Language::Shell => tree_sitter_bash::LANGUAGE.into(),
        };
        parser.set_language(&grammar)?;
        Ok(Self {
            call_id,
            language,
            cwd,
            source: String::new(),
            revision: Revision::default(),
            parser,
            tree: None,
            analyzer: Analyzer::new()?,
            failed: false,
        })
    }

    /// Exceeding the byte budget aborts the preview. It cannot then be prepared.
    ///
    /// # Errors
    /// Returns an error for an aborted preview, excessive source size or parser failure.
    pub fn append(&mut self, fragment: &str) -> Result<PendingActivity, ActivityError> {
        if self.failed {
            return Err(ActivityError::StreamAborted);
        }
        if fragment.len() > MAX_SOURCE_BYTES.saturating_sub(self.source.len()) {
            self.failed = true;
            return Err(ActivityError::SourceLimit);
        }
        let old_end_byte = self.source.len();
        let old_end_position = end_position(&self.source);
        self.source.push_str(fragment);
        if let Some(tree) = &mut self.tree {
            tree.edit(&InputEdit {
                start_byte: old_end_byte,
                old_end_byte,
                new_end_byte: self.source.len(),
                start_position: old_end_position,
                old_end_position,
                new_end_position: end_position(&self.source),
            });
        }
        self.tree = self.parser.parse(&self.source, self.tree.as_ref());
        if self.tree.is_none() {
            self.failed = true;
            return Err(ActivityError::ParseInterrupted);
        }
        self.revision.0 += 1;
        let (analyzed_bytes, syntax_complete) = self.tree.as_ref().map_or((0, false), |tree| {
            let root = tree.root_node();
            if !root.has_error() {
                return (self.source.len(), true);
            }
            // Stop before the first erroneous top-level construct. Do not join
            // disjoint valid islands: that would bypass invalidated bindings.
            let mut cursor = root.walk();
            let mut end = 0;
            for child in root.children(&mut cursor) {
                if child.has_error() || child.is_error() || child.is_missing() {
                    break;
                }
                end = child.end_byte();
            }
            (end, false)
        });
        // Incremental CST parsing chooses the prefix. Semantic analysis is
        // recomputed from the prefix so no stale bindings survive a revision.
        let report = self.analyzer.analyze(Request {
            call_id: self.call_id.clone(),
            language: self.language,
            cwd: self.cwd.clone(),
            source: self.source[..analyzed_bytes].into(),
            snapshots: vec![],
        });
        Ok(PendingActivity {
            phase: PendingPhase::Pending,
            revision: self.revision,
            source_bytes: self.source.len(),
            analyzed_bytes,
            syntax_complete,
            report,
        })
    }

    /// Final syntax must stand alone. An invalid final cell retracts its preview.
    ///
    /// # Errors
    /// Returns an error if the preview was aborted by an earlier append failure.
    pub fn finish(mut self) -> Result<FinalCall, ActivityError> {
        if self.failed {
            return Err(ActivityError::StreamAborted);
        }
        Ok(FinalCall {
            report: self.analyzer.analyze(Request {
                call_id: self.call_id,
                language: self.language,
                cwd: self.cwd,
                source: self.source,
                snapshots: vec![],
            }),
        })
    }
}

fn end_position(source: &str) -> Point {
    Point {
        row: source.bytes().filter(|b| *b == b'\n').count(),
        column: source.rsplit('\n').next().unwrap_or("").len(),
    }
}
