//! Caller-supplied snapshot identities and text-I/O premises.

use super::SnapshotPreviewLimits;
use crate::CallId;
use crate::SourceId;
use crate::Target;
use crate::selective_edits::TextEncoding;
use serde::Deserialize;
use serde::Serialize;

/// A read identity within one source in the input's call namespace.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct SourceReadId {
    /// Source-table identity, never an operation ordinal.
    pub source_id: SourceId,
    /// Source-local read ordinal.
    pub read_id: usize,
}

impl SourceReadId {
    /// Associates a producer's source ID with its source-local read ordinal.
    #[must_use]
    pub const fn new(source_id: SourceId, read_id: usize) -> Self {
        Self { source_id, read_id }
    }
}

/// Number of preceding modeled mutations within the associated source.
/// This is an ordering identity, not a filesystem timestamp or freshness check.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq, Hash)]
#[serde(transparent)]
pub struct SnapshotEpoch(usize);

impl SnapshotEpoch {
    /// Creates a caller assertion about a source-local mutation epoch.
    #[must_use]
    pub const fn new(value: usize) -> Self {
        Self(value)
    }

    /// Returns the source-local mutation count.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

/// Effective codec supported by this bounded materializer.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum SnapshotEncoding {
    /// Strict UTF-8; invalid bytes are refused rather than replaced.
    Utf8,
}

/// Trusted decoding behavior for a supplied read snapshot.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum SnapshotReadNewline {
    /// No behavior has been supplied; reads requiring it are refused.
    #[default]
    Unspecified,
    /// Python text reads translate CRLF and bare CR to LF.
    PythonUniversal,
    /// Resolve each selected read from its source-declared policy.
    SourceDeclared,
    /// Node UTF-8 reads preserve every decoded newline character.
    Preserve,
}

/// Trusted encoding behavior for text supplied to a write operation.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum SnapshotWriteNewline {
    /// No behavior has been supplied; text writes are refused.
    #[default]
    Unspecified,
    /// Python's default text writer uses an LF host line separator.
    Lf,
    /// Python's default text writer replaces every LF with CRLF.
    CrLf,
    /// Replace each LF with CR under an explicit source policy.
    Cr,
    /// Node encodes the supplied string without newline translation.
    Preserve,
}

/// Explicit host premises; constructing this value does not attest a runtime.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[non_exhaustive]
pub struct SnapshotTextProfile {
    /// Read newline semantics supplied by the caller.
    pub read_newline: SnapshotReadNewline,
    /// Write newline semantics supplied by the caller.
    pub write_newline: SnapshotWriteNewline,
    /// Effective codec for omitted Python encodings; absent means unresolved.
    pub runtime_default_encoding: Option<SnapshotEncoding>,
}

impl SnapshotTextProfile {
    /// Creates an explicit newline profile with an unresolved default codec.
    #[must_use]
    pub const fn new(
        read_newline: SnapshotReadNewline,
        write_newline: SnapshotWriteNewline,
    ) -> Self {
        Self {
            read_newline,
            write_newline,
            runtime_default_encoding: None,
        }
    }

    /// Adds a trusted host premise resolving omitted Python encoding arguments.
    #[must_use]
    pub const fn with_runtime_default_encoding(mut self, encoding: SnapshotEncoding) -> Self {
        self.runtime_default_encoding = Some(encoding);
        self
    }
}

/// Exact producer identity that a trusted read snapshot is asserted to match.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct SnapshotReadBinding {
    /// Call-local source and source-local read identity.
    pub read: SourceReadId,
    /// Mutation count at the source read.
    pub filesystem_epoch: SnapshotEpoch,
    /// Exact source spelling and cwd; aliases are never resolved.
    pub target: Target,
    /// Source-declared codec, including an explicitly unresolved default.
    pub encoding: TextEncoding,
    /// Trusted newline behavior used to decode these bytes.
    pub newline: SnapshotReadNewline,
}

impl SnapshotReadBinding {
    /// Records caller assertions that are checked against the selected recipe.
    #[must_use]
    pub const fn new(
        read: SourceReadId,
        filesystem_epoch: SnapshotEpoch,
        target: Target,
        encoding: TextEncoding,
        newline: SnapshotReadNewline,
    ) -> Self {
        Self {
            read,
            filesystem_epoch,
            target,
            encoding,
            newline,
        }
    }
}

/// Bytes vouched for by the caller at one particular read operation point.
/// No file is opened, and freshness cannot be established from this assertion.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct TrustedReadSnapshot {
    /// Exact read binding and decoding premises.
    pub binding: SnapshotReadBinding,
    /// Original file bytes before text decoding.
    pub bytes: Vec<u8>,
}

impl TrustedReadSnapshot {
    /// Supplies bytes and their trusted operation-point binding without I/O.
    #[must_use]
    pub const fn new(binding: SnapshotReadBinding, bytes: Vec<u8>) -> Self {
        Self { binding, bytes }
    }
}

/// Exact destination identity immediately before the selected mutation.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct SnapshotTargetBinding {
    /// Source owning the selected mutation.
    pub source_id: SourceId,
    /// Source-local mutation count immediately before the selected operation.
    pub filesystem_epoch: SnapshotEpoch,
    /// Exact target spelling and cwd, without path normalization.
    pub target: Target,
}

impl SnapshotTargetBinding {
    /// Records the operation point for the destination preimage.
    #[must_use]
    pub const fn new(source_id: SourceId, filesystem_epoch: SnapshotEpoch, target: Target) -> Self {
        Self {
            source_id,
            filesystem_epoch,
            target,
        }
    }
}

/// A caller-vouched-for destination preimage at the selected operation point.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct TrustedTargetSnapshot {
    /// Operation-point identity of the preimage.
    pub binding: SnapshotTargetBinding,
    /// Original bytes, or absence of the file; absence never means unreadable.
    pub bytes: Option<Vec<u8>>,
}

impl TrustedTargetSnapshot {
    /// Supplies a destination preimage without inspecting its path.
    #[must_use]
    pub const fn new(binding: SnapshotTargetBinding, bytes: Option<Vec<u8>>) -> Self {
        Self { binding, bytes }
    }
}

/// Trusted external inputs for one conditional operation-point preview.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct SnapshotPreviewInput {
    /// Namespaces every supplied source/read ID to this analysis call.
    pub call_id: CallId,
    /// Destination bytes immediately before the selected source operation.
    pub before: TrustedTargetSnapshot,
    /// Read snapshots; only the selected recipe branch is consumed.
    pub reads: Vec<TrustedReadSnapshot>,
    /// Trusted text-I/O behavior, separate from source/API assumptions.
    pub text_profile: SnapshotTextProfile,
    /// Resource ceilings; values above hard ceilings are clamped.
    pub limits: SnapshotPreviewLimits,
}

impl SnapshotPreviewInput {
    /// Creates input with no read snapshots and unresolved I/O premises.
    #[must_use]
    pub fn new(call_id: CallId, before: TrustedTargetSnapshot) -> Self {
        Self {
            call_id,
            before,
            reads: Vec::new(),
            text_profile: SnapshotTextProfile::default(),
            limits: SnapshotPreviewLimits::default(),
        }
    }
}
