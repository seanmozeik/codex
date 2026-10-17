//! Requested patch syntax has its own byte-coordinate domain and no effect API.

use crate::Span;
use serde::Serialize;

/// Index in a complete description's patch table, independent of SourceId.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct PatchId(pub(super) usize);

impl PatchId {
    /// Zero-based index in the patch table.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0
    }
}

/// A literal patch-header path, with no resolution or filesystem authority.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct LiteralPatchPath(pub(super) String);

impl LiteralPatchPath {
    /// Exact source spelling, without normalization or filesystem lookup.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Half-open UTF-8 bytes in a cooked patch payload, never JavaScript bytes.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct PatchByteRange {
    /// Inclusive zero-based byte offset.
    pub start: usize,
    /// Exclusive zero-based byte offset.
    pub end: usize,
}

/// Location in a decoded patch, whose original argument is separate provenance.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct PatchEvidence {
    /// Owning patch table entry.
    pub patch_id: PatchId,
    /// UTF-8 byte range in that entry's payload.
    pub range: PatchByteRange,
}

/// Path spelling and its exact range after the patch header prefix.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct PatchPathReference {
    /// Source spelling only, not a verified or resolved path.
    pub spelling: LiteralPatchPath,
    /// Range excludes the header prefix and final LF.
    pub evidence: PatchEvidence,
}

/// The submitted line marker, not an observed file operation.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PatchLinePrefix {
    /// A space-prefixed context line.
    Context,
    /// A plus-prefixed requested line.
    Plus,
    /// A minus-prefixed requested line.
    Minus,
}

/// One exact submitted patch line, retained by range rather than copying text.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct PatchLine {
    /// Submitted marker.
    pub prefix: PatchLinePrefix,
    /// Complete line, including its marker and excluding LF.
    pub evidence: PatchEvidence,
    /// Line remainder, excluding its marker and LF; may be empty.
    pub text: PatchEvidence,
}

/// A completely recognized update chunk, with its submitted line ordering.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct PatchChunk {
    /// Complete @@ marker line, if the initial chunk has an explicit marker.
    pub marker: Option<PatchEvidence>,
    /// Nonempty source lines in submitted order.
    pub lines: Vec<PatchLine>,
    /// Submitted terminal End-of-File marker, if present.
    pub end_of_file: Option<PatchEvidence>,
}

/// One source directive. Repeated paths remain distinct ordered directives.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[non_exhaustive]
pub enum SourcePatchFile {
    /// An Add header and its nonempty plus-prefixed body.
    Add {
        /// Header path spelling.
        path: PatchPathReference,
        /// Requested lines, not file contents observed on disk.
        lines: Vec<PatchLine>,
    },
    /// A Delete header with no body.
    Delete {
        /// Header path spelling.
        path: PatchPathReference,
    },
    /// An Update header, optional Move header, and nonempty chunks.
    Update {
        /// Original header path spelling.
        path: PatchPathReference,
        /// Optional submitted destination spelling, without path resolution.
        #[serde(rename = "moveTo")]
        move_to: Option<PatchPathReference>,
        /// Requested chunks, not a verified or applied diff.
        chunks: Vec<PatchChunk>,
    },
}

/// A fully parsed requested patch in a completely recognized outer call.
/// This carries no execution status, snapshots, resolved paths or effects.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct SourcePatch {
    /// Index in the source-only patch table.
    pub id: PatchId,
    /// Complete tool call in the original source table.
    pub invocation: Span,
    /// Original literal argument, including quotes, in the source table.
    pub argument: Span,
    /// Exact cooked patch text. All patch evidence indexes these UTF-8 bytes.
    pub payload: String,
    /// Nonempty ordered, completely validated source directives.
    pub files: Vec<SourcePatchFile>,
}
