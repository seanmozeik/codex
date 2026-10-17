//! Source-declared policies and identities for bounded owned text streams.

use super::TextEncoding;
use super::TextRecipe;
use crate::Span;
use serde::Serialize;

/// Decoding policy explicitly selected by the source for a full text read.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum PythonReadNewline {
    /// Translate CRLF and bare CR to LF.
    UniversalTranslate,
    /// Preserve all decoded characters.
    Preserve,
}

/// Output policy explicitly selected by the source.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum PythonWriteNewline {
    /// A trusted host separator is still required.
    RuntimeDefault,
    /// Encode without newline translation.
    Preserve,
    /// Replace each LF with CR.
    Cr,
    /// Replace each LF with CRLF.
    CrLf,
}

/// Source-local owned open identity, separate from bindings and file epochs.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq, Hash)]
#[serde(transparent)]
pub struct PythonStreamId(usize);

impl PythonStreamId {
    /// Creates a source-local stream identity.
    #[must_use]
    pub const fn new(value: usize) -> Self {
        Self(value)
    }

    /// Returns the source-local open generation.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

/// The two owned write modes supported by the logical stream lane.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum PythonStreamMode {
    /// A successful preceding open truncated the seed to empty bytes.
    TruncateWrite,
    /// An existing seed is overwritten without truncating its untouched suffix.
    UpdateExisting,
}

/// Proof of the zero position; never a Python numeric tell cookie.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[non_exhaustive]
pub enum PythonStreamPosition {
    /// No preceding read or seek changed the initial position.
    InitialZero,
    /// A successful full read was followed by successful literal seek(0).
    RewoundZero {
        /// Captured source-local read identity.
        read_id: usize,
        /// Full read call whose completion precedes the seek.
        read_evidence: Span,
        /// Exact literal seek(0) call.
        seek_evidence: Span,
    },
}

/// Required current-call result, not an observed return value.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum PythonWriteResultCondition {
    /// All pre-translation Unicode characters must be accepted.
    FullCharacterCount,
}

/// One conditional logical write, with no immediate file-visibility claim.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct PythonLogicalWrite {
    /// Source-local owned open identity.
    pub stream_id: PythonStreamId,
    /// The successful open call required to obtain the owned stream.
    pub opened_at: Span,
    /// Opening mode, distinct from the selected position.
    pub mode: PythonStreamMode,
    /// Proven zero-position origin.
    pub position: PythonStreamPosition,
    /// Physical seed epoch; not the logical operation ordinal.
    pub base_filesystem_epoch: usize,
    /// Logical revision immediately before acceptance; zero in this registry.
    pub stream_revision: usize,
    /// Frozen payload expression.
    pub content: TextRecipe,
    /// Source encoding; this owned registry requires explicit UTF-8.
    pub encoding: TextEncoding,
    /// Explicit source output policy.
    pub newline: PythonWriteNewline,
    /// Conditional full-acceptance requirement for this write.
    pub required_result: PythonWriteResultCondition,
}
