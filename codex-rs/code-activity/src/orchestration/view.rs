//! Whole-envelope atomic presentation, with child-local source evidence.

use crate::CallId;
use crate::FallbackReason;
use crate::Span;
use crate::WholeScriptActivity;
use serde::Serialize;

/// Child ordinal scoped to the enclosing orchestration call identity.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct OrchestrationChildId(pub(super) usize);

impl OrchestrationChildId {
    /// Returns the source-order ordinal, not a runtime dispatch identifier.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0
    }
}

/// Awaited source grouping, never proof of process completion or file ordering.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum OrchestrationJoin {
    /// One awaited host tool response; the underlying process can still be live.
    Single,
    /// Native Promise.all over direct calls; children may overlap and fail.
    All,
    /// Native Promise.allSettled over direct calls; settlement is not success.
    AllSettled,
}

/// Closed reasons for discarding the complete envelope's rich presentation.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum OrchestrationFallback {
    /// Outer bindings or child runtime assumptions were not established.
    EnvironmentUnknown,
    /// Syntax or a construct was outside the finite orchestration grammar.
    UnsupportedSource,
    /// A child, binding, string, group or traversal limit was exceeded.
    ResourceLimit,
    /// The cooperative budget shared by extraction and all children expired.
    TimeBudget,
    /// A whole shell child could not be interpreted completely.
    Child(FallbackReason),
}

/// Eligibility for provisional intent only; never an authorization decision.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "type", content = "reason", rename_all = "camelCase")]
#[non_exhaustive]
pub enum OrchestrationDisposition {
    /// Every admitted construct and complete child passed the strict gate.
    Eligible,
    /// No group, child effect or diff survives.
    Fallback(OrchestrationFallback),
}

/// One complete shell child's provisional effects, after the entire envelope clears.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrchestrationChild {
    pub(super) id: OrchestrationChildId,
    pub(super) call_span: Span,
    pub(super) command_span: Span,
    #[serde(skip)]
    pub(super) command: String,
    pub(super) activity: WholeScriptActivity,
}

impl OrchestrationChild {
    /// Parent-scoped child identity, unrelated to runtime tool invocation IDs.
    #[must_use]
    pub const fn id(&self) -> OrchestrationChildId {
        self.id
    }
    /// Callsite span in the original outer source (source ID zero).
    #[must_use]
    pub fn call_span(&self) -> &Span {
        &self.call_span
    }
    /// Command argument expression span in the outer source, possibly a binding.
    #[must_use]
    pub fn command_span(&self) -> &Span {
        &self.command_span
    }
    /// Exact decoded complete shell carrier supplied to the child analyzer.
    /// Display-only evidence; never substitute this for original host dispatch.
    #[must_use]
    pub fn command(&self) -> &str {
        &self.command
    }
    /// Complete-child intent with its own source table; never supplied snapshots.
    #[must_use]
    pub fn activity(&self) -> &WholeScriptActivity {
        &self.activity
    }
}

/// A source-level join group. Later groups can remain uninvoked after errors.
#[derive(Debug, Serialize)]
pub struct OrchestrationGroup {
    pub(super) join: OrchestrationJoin,
    pub(super) span: Span,
    pub(super) children: Vec<OrchestrationChild>,
}

impl OrchestrationGroup {
    /// Source-level Promise join semantics, not observed completion order.
    #[must_use]
    pub const fn join(&self) -> OrchestrationJoin {
        self.join
    }
    /// Await expression in the enclosing original source.
    #[must_use]
    pub fn span(&self) -> &Span {
        &self.span
    }
    /// Children in source invocation order, without an inter-child effect order.
    #[must_use]
    pub fn children(&self) -> &[OrchestrationChild] {
        &self.children
    }
}

/// Immutable whole-envelope presentation. Fallback always has zero groups.
/// Original source is accessible but omitted from serialization; callers retain
/// the original host dispatch input. No filesystem observation takes place.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrchestrationActivity {
    pub(super) call_id: CallId,
    #[serde(skip)]
    pub(super) source: String,
    pub(super) disposition: OrchestrationDisposition,
    pub(super) groups: Vec<OrchestrationGroup>,
}

impl OrchestrationActivity {
    /// Original outer host identity, unchanged on fallback.
    #[must_use]
    pub fn call_id(&self) -> &CallId {
        &self.call_id
    }
    /// Exact original complete source, unchanged on fallback.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }
    /// Atomic presentation decision, not a security or execution verdict.
    #[must_use]
    pub const fn disposition(&self) -> OrchestrationDisposition {
        self.disposition
    }
    /// All admitted groups or an empty slice on every fallback.
    #[must_use]
    pub fn groups(&self) -> &[OrchestrationGroup] {
        &self.groups
    }
}
