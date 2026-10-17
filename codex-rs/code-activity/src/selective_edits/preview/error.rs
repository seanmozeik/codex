//! Typed refusals distinguish unavailable evidence from recipe exceptions.

use super::SourceReadId;
use crate::Span;
use serde::Serialize;
use thiserror::Error;

/// Resource category that prevented a complete exact preview.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum SnapshotPreviewLimit {
    /// Supplied bytes, metadata or snapshot-record count.
    Input,
    /// Any intermediate or encoded output string.
    Output,
    /// Cumulative scan, allocation and evaluation work.
    Work,
    /// Evaluated recipe-node count.
    Nodes,
    /// Evaluated recipe nesting depth.
    Depth,
    /// Serialized unified-diff bytes.
    Diff,
}

/// Why no complete conditional artifact was produced.
#[derive(Clone, Debug, Error, Serialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[non_exhaustive]
pub enum SnapshotPreviewError {
    /// The requested ordinal is absent or the analysis is unavailable.
    #[error("selected operation is unavailable")]
    OperationUnavailable,
    /// The operation is outside this materializer's finite recipe registry.
    #[error("selected operation has no supported text recipe")]
    UnsupportedOperation,
    /// Snapshot identities were supplied for a different call.
    #[error("snapshot call identity does not match the analysis")]
    CallMismatch,
    /// The destination source, epoch or exact path/cwd differs.
    #[error("destination snapshot binding does not match the operation point")]
    TargetBindingMismatch,
    /// An open descriptor requires an existing file at its write operation point.
    #[error("descriptor write requires an existing destination snapshot")]
    MissingDescriptorPreimage,
    /// The supplied stream open generation, revision or physical seed differs.
    #[error("logical stream identity does not match the selected write")]
    LogicalStreamBindingMismatch,
    /// Logical stream materialization requires a present physical seed.
    #[error("logical stream requires a present seed snapshot")]
    MissingLogicalStreamPreimage,
    /// A truncate-mode stream must start with the already-truncated empty seed.
    #[error("logical stream seed contradicts its opening mode")]
    LogicalStreamSeedMismatch,
    /// Two snapshots claim the same source-local read identity.
    #[error("duplicate snapshot for read {read:?}")]
    DuplicateRead {
        /// Ambiguous read identity.
        read: SourceReadId,
    },
    /// A selected read has no supplied snapshot in this source namespace.
    #[error("missing snapshot for read {read:?}")]
    MissingRead {
        /// Required read identity.
        read: SourceReadId,
    },
    /// The read exists, but its epoch, target, codec or newline binding differs.
    #[error("snapshot binding does not match read {read:?}")]
    ReadBindingMismatch {
        /// Read whose supplied metadata differs.
        read: SourceReadId,
    },
    /// Same-target, same-epoch supplied snapshots contradict each other.
    #[error("supplied snapshots contradict read {read:?}")]
    ConflictingSnapshots {
        /// Read contradicting another supplied snapshot.
        read: SourceReadId,
    },
    /// An omitted Python encoding has no explicit trusted host resolution.
    #[error("runtime-default encoding requires an explicit trusted profile")]
    UnresolvedEncoding,
    /// The profile lacks or contradicts the language's newline semantics.
    #[error("newline semantics are unspecified or incompatible")]
    NewlineProfileMismatch,
    /// Strict UTF-8 decoding failed; no replacement text is guessed.
    #[error("supplied snapshot is not valid utf-8")]
    InvalidUtf8,
    /// Python's substring-index operation would raise ValueError.
    #[error("substring index has no match in the supplied text")]
    SubstringNotFound {
        /// Source expression whose conditional evaluation failed.
        evidence: Span,
    },
    /// Python's split subscription would raise IndexError.
    #[error("split item index is outside the supplied text's result")]
    SplitIndexOutOfRange {
        /// Source subscription whose conditional evaluation failed.
        evidence: Span,
    },
    /// An explicit empty split separator would raise ValueError.
    #[error("split separator is empty")]
    EmptySeparator,
    /// A finite work or allocation bound would be exceeded.
    #[error("snapshot preview resource limit exceeded: {limit:?}")]
    ResourceLimit {
        /// Exhausted resource category.
        limit: SnapshotPreviewLimit,
    },
}
