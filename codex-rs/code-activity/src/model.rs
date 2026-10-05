//! Requests and evidence produced from source interpretation.

use crate::CallId;
use crate::OperationId;
use crate::ReportVersion;
use crate::SourceId;
use crate::Transform;
use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// Grammar used to interpret a decoded payload.
#[non_exhaustive]
pub enum Language {
    /// Python source, including supported REPL cells.
    Python,
    /// TypeScript or JavaScript source.
    TypeScript,
    /// Shell source parsed with the Bash grammar.
    Shell,
}

/// One complete cell or tool payload. Bindings never carry across requests.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
/// A complete source request and optional host-supplied endpoint pairs.
#[non_exhaustive]
pub struct Request {
    /// Opaque host-assigned call identity.
    pub call_id: CallId,
    /// Grammar for this source.
    pub language: Language,
    /// Decoded source, not transport JSON.
    pub source: String,
    /// Working directory in the execution environment, if known.
    pub cwd: Option<String>,
    #[serde(default)]
    /// Optional host-supplied text pairs; the analyser does not read their paths.
    pub snapshots: Vec<FileSnapshot>,
}

/// Content supplied by the host. The analyser never reads these paths.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
/// Text endpoints; absent contents mean a missing file, not an unreadable file.
#[non_exhaustive]
pub struct FileSnapshot {
    /// Literal or display path; it may contain filesystem aliases.
    pub path: String,
    /// Original text; absent means the file did not exist.
    pub before: Option<String>,
    /// Final text; absent means the file did not exist.
    pub after: Option<String>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// Replacement view of source intent, unresolved regions and supplied comparisons.
#[non_exhaustive]
pub struct Report {
    /// Version of the activity wire format.
    pub version: ReportVersion,
    /// Opaque host-assigned call identity.
    pub call_id: CallId,
    /// Extent of source recognition.
    pub coverage: Coverage,
    /// Decoded source table for evidence spans.
    pub sources: Vec<Source>,
    /// Operations recognised in this report revision.
    pub operations: Vec<Operation>,
    /// Unsupported regions and analysis limits.
    pub unresolved: Vec<Unresolved>,
    /// Comparisons of the supplied host snapshots.
    pub diffs: Vec<FileDiff>,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// Whether useful operations were recognised; never a claim of complete analysis.
#[non_exhaustive]
pub enum Coverage {
    /// Some operations were recognised; other effects may exist.
    Partial,
    /// No operations were recognised.
    Opaque,
}

/// Offsets are UTF-8 bytes; lines and columns are one-based. Columns are byte columns.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// A range in a decoded source, using UTF-8 byte offsets.
#[non_exhaustive]
pub struct Span {
    /// Index in the report source table.
    pub source_id: SourceId,
    /// Inclusive zero-based byte offset.
    pub start_byte: usize,
    /// Exclusive zero-based byte offset.
    pub end_byte: usize,
    /// One-based starting line.
    pub start_line: usize,
    /// One-based ending line.
    pub end_line: usize,
    /// One-based starting byte column.
    pub start_column: usize,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// Provenance of an outer or embedded source.
#[non_exhaustive]
pub struct Source {
    /// Index in the associated report table.
    pub id: SourceId,
    /// Grammar for this source.
    pub language: Language,
    /// Containing argument or heredoc span; absent for the outer source.
    pub parent: Option<Span>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase")]
/// File location inferred from source, without filesystem resolution.
#[non_exhaustive]
pub enum Target {
    /// A literal path recovered through supported data flow.
    Literal {
        /// Literal or display path; it may contain filesystem aliases.
        path: String,
        /// Working directory in the execution environment, if known.
        cwd: Option<String>,
    },
    /// The operation was recognised but its target was not resolved.
    Unresolved,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// One source-derived operation, without an execution claim.
#[non_exhaustive]
pub struct Operation {
    /// Index in the associated report table.
    pub id: OperationId,
    /// Source span supporting this record.
    pub evidence: Span,
    /// These are source interpretations, never assertions of execution success.
    /// Interpretation evidence; does not prove execution.
    pub basis: Basis,
    /// The recognised operation and its arguments.
    pub effect: Effect,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// Evidence supporting an operation record.
#[non_exhaustive]
pub enum Basis {
    /// Inferred from source under the documented interpreter assumptions.
    StaticIntent,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
/// Recognised source effect and its unresolved or literal arguments.
#[non_exhaustive]
pub enum Effect {
    /// Remove a file or directory. This is intent, not observed deletion.
    FileDelete {
        /// Inferred destination location.
        target: Target,
    },
    /// Explicitly change a file's length; the resulting bytes are not inferred.
    FileTruncate {
        /// Inferred destination location.
        target: Target,
    },
    /// Opening a file in a mutating mode can change it without a later write.
    /// Opening a file may mutate it before any later write call.
    FileOpen {
        /// Inferred source or destination location.
        target: Target,
        /// Inferred write or open mode.
        mode: WriteMode,
    },
    /// Read file contents.
    FileRead {
        /// Inferred source or destination location.
        target: Target,
    },
    /// Write data without a recognised same-file transformation.
    FileWrite {
        /// Inferred source or destination location.
        target: Target,
        /// Inferred write or open mode.
        mode: WriteMode,
    },
    /// Write transformed content back to its known source target.
    FileEdit {
        /// Inferred source or destination location.
        target: Target,
        /// Recognised transformations; these are not evaluated patches.
        transforms: Vec<Transform>,
    },
    /// List entries under a directory or pattern root.
    FileList {
        /// Inferred source or destination location.
        target: Target,
    },
    /// Launch a child process whose other effects are unknown.
    ProcessRun {
        /// Literal arguments; each unknown argument is absent.
        argv: Vec<Option<String>>,
        /// Shell command text, when the source API uses a shell.
        shell: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// Known or unresolved semantics of file opening and writing.
#[non_exhaustive]
pub enum WriteMode {
    /// Replace or truncate existing contents.
    Replace,
    /// Append to existing contents.
    Append,
    /// Create only if the file does not exist.
    ExclusiveCreate,
    /// Options prevent a more specific interpretation.
    Unknown,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// A region where source analysis abstained.
#[non_exhaustive]
pub struct Unresolved {
    /// Source span supporting this record.
    pub evidence: Span,
    /// Human-readable explanation of this limitation.
    pub reason: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
/// An endpoint comparison or explicit reason it was omitted.
#[non_exhaustive]
pub enum FileDiff {
    /// The file-count budget omitted remaining comparisons.
    Truncated {
        /// Number of comparisons omitted by the file-count budget.
        omitted_files: usize,
    },
    /// Both text endpoints were available for comparison.
    Compared {
        /// Literal or display path; it may contain filesystem aliases.
        path: String,
        /// Whether existence or text differed between endpoints.
        changed: bool,
        /// Unified text diff; empty-file creation may have no textual hunk.
        unified_diff: String,
        /// Snapshot provenance, without proof of writer identity.
        attribution: DiffAttribution,
    },
    /// The comparison exceeded a supported limit.
    Omitted {
        /// Literal or display path; it may contain filesystem aliases.
        path: String,
        /// Human-readable explanation of this limitation.
        reason: String,
    },
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// Evidence used to produce a file comparison.
#[non_exhaustive]
pub enum DiffAttribution {
    /// Supplied endpoints only; the writer is not established.
    HostSnapshotsOnly,
}

impl Request {
    /// Creates a source request without supplied snapshots.
    #[must_use]
    pub const fn new(
        call_id: CallId,
        language: Language,
        source: String,
        cwd: Option<String>,
    ) -> Self {
        Self {
            call_id,
            language,
            source,
            cwd,
            snapshots: Vec::new(),
        }
    }
    /// Attaches host-supplied endpoint comparisons.
    #[must_use]
    pub fn with_snapshots(mut self, snapshots: Vec<FileSnapshot>) -> Self {
        self.snapshots = snapshots;
        self
    }
}

impl FileSnapshot {
    /// Creates a pair of text endpoints; absent means missing, not unreadable.
    #[must_use]
    pub const fn new(path: String, before: Option<String>, after: Option<String>) -> Self {
        Self {
            path,
            before,
            after,
        }
    }
}
