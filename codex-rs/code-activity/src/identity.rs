//! Distinct identifiers for calls, syntax sources and reported operations.

use serde::Deserialize;
use serde::Serialize;

/// An opaque host-assigned call identifier. Its contents have no path semantics.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(transparent)]
pub struct CallId(String);

impl From<String> for CallId {
    fn from(value: String) -> Self {
        Self(value)
    }
}
impl From<&str> for CallId {
    fn from(value: &str) -> Self {
        Self(value.into())
    }
}
impl CallId {
    /// Returns the host identifier unchanged.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Index of a decoded source within one report.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq, Hash)]
#[serde(transparent)]
pub struct SourceId(pub(crate) usize);

impl SourceId {
    /// Returns the source-table index; it is unrelated to operation IDs.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0
    }
}

/// Index of an operation within one report revision.
///
/// Source and operation IDs cannot be substituted for each other:
///
/// ```compile_fail
/// use codex_code_activity::{OperationId, SourceId};
/// fn mix_ids(source: SourceId) -> OperationId { source }
/// ```
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq, Hash)]
#[serde(transparent)]
pub struct OperationId(pub(crate) usize);

impl OperationId {
    /// Returns the operation-table index within the associated report.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0
    }
}

/// A monotonically increasing revision within one pending stream.
#[derive(Clone, Copy, Debug, Default, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(transparent)]
pub struct Revision(pub(crate) u64);

impl Revision {
    /// Returns the revision counter for ordering replacement views.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}
