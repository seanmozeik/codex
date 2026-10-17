//! Conditional Node CommonJS recipes, separate from code-mode orchestration.

mod declarations;
mod eval;
mod syntax;

use crate::SourceDescriptionFallback;
use crate::SourceId;
use crate::Span;
use crate::Target;
use crate::source_description::DescriptionBudget;
use serde::Serialize;
use tree_sitter::Parser;

/// Ordered conditional operations from a complete, bounded CommonJS source.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JavaScriptPlan {
    /// Synchronous truncations and UTF-8 writes in source evaluation order.
    pub edits: Vec<JavaScriptEdit>,
    /// First unsupported execution point, if present.
    pub barriers: Vec<JavaScriptBarrier>,
    /// Explicit standard-runtime hypotheses, never runtime attestations.
    pub assumptions: Vec<JavaScriptAssumption>,
    /// Earlier modeled module loads and I/O required to complete normally.
    pub completion_conditions: Vec<Span>,
}

/// One conditional synchronous file effect, without observed file bytes.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JavaScriptEdit {
    /// Zero-based write order within this source unit.
    pub ordinal: usize,
    /// Exact write call in the owned CommonJS source.
    pub evidence: Span,
    /// Literal path spelling and entry cwd, without filesystem resolution.
    pub target: Target,
    /// UTF-8 string supplied to the write, or empty text for an open-time truncation.
    pub content: JavaScriptTextRecipe,
    /// Whether the content replaces, overlays or appends file bytes.
    /// Ordinary path writes retain their original serialized shape.
    #[serde(skip_serializing_if = "JavaScriptWriteEffect::is_overwrite")]
    pub write_effect: JavaScriptWriteEffect,
    /// Frozen bindings, reads, environment and capabilities used by this write.
    pub dependencies: Vec<JavaScriptDependency>,
    /// Modeled predecessors whose normal completion is required.
    pub predecessor_conditions: Vec<Span>,
}

/// File mutation semantics, separate from the string argument's recipe.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[non_exhaustive]
pub enum JavaScriptWriteEffect {
    /// A normal path-based `writeFileSync` replaces the complete file.
    Overwrite,
    /// Opening with `w` or `w+` truncates the file before any descriptor write.
    Truncate {
        /// Source-local descriptor identity, never an operating-system fd number.
        descriptor_id: usize,
        /// Canonical opening call that owns this descriptor.
        opened_at: Span,
    },
    /// Overlay UTF-8 bytes, preserving the suffix and zero-filling any gap.
    /// Exact bytes are conditional on `CompleteDescriptorWrites`; a normal
    /// `writeSync` return alone does not establish full-byte completion.
    DescriptorWrite {
        /// Source-local descriptor identity, never an operating-system fd number.
        descriptor_id: usize,
        /// Canonical opening call that owns this descriptor.
        opened_at: Span,
        /// Resolved byte offset, independent of Unicode scalar or UTF-16 indices.
        byte_offset: usize,
        /// Whether this call consumes the cursor or leaves it unchanged.
        position: JavaScriptDescriptorPosition,
    },
    /// Append UTF-8 bytes at this operation's EOF, without a source-level offset.
    /// Materialization requires a present operation-point preimage. Exact bytes
    /// remain conditional on `CompleteDescriptorWrites`, not merely a return.
    DescriptorAppend {
        /// Source-local descriptor identity, shared by its immutable aliases.
        descriptor_id: usize,
        /// Canonical append-mode opening call that owns this descriptor.
        opened_at: Span,
    },
}

impl JavaScriptWriteEffect {
    fn is_overwrite(&self) -> bool {
        matches!(self, Self::Overwrite)
    }
}

/// The position argument's effect on the descriptor's own cursor.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum JavaScriptDescriptorPosition {
    /// An omitted or null position advances the cursor by bytes actually written.
    Cursor,
    /// An explicit nonnegative byte offset leaves the cursor unchanged.
    Absolute,
}

/// Immutable JavaScript string expressions; no Python string semantics apply.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[non_exhaustive]
pub enum JavaScriptTextRecipe {
    /// Literal scalar text, or the empty placeholder for an open-time truncation.
    Literal {
        /// Decoded text; unsupported lone-surrogate forms are refused.
        value: String,
        /// Original literal token, or the opening call for a truncation placeholder.
        evidence: Span,
    },
    /// A captured Node UTF-8 read, without Python newline conversion.
    ReadUtf8 {
        /// Identity local to the source identified by `evidence.source_id`.
        read_id: usize,
        /// Literal path spelling and entry cwd.
        target: Target,
        /// Exact read call.
        evidence: Span,
        /// Number of preceding modeled writes in this source.
        filesystem_epoch: usize,
    },
    /// First string match only; replacement text contains no dollar sign.
    ReplaceFirst {
        /// Captured primitive-string receiver.
        input: Box<Self>,
        /// Literal-origin search string, including the empty string.
        old: String,
        /// Literal-origin replacement; JavaScript substitution syntax is refused.
        new: String,
        /// Complete replacement call.
        evidence: Span,
    },
    /// All nonoverlapping string matches; needle is nonempty and replacement is literal.
    ReplaceAll {
        /// Captured primitive-string receiver containing Unicode scalar values only.
        input: Box<Self>,
        /// Nonempty literal-origin search string.
        old: String,
        /// Literal-origin replacement containing no dollar sign.
        new: String,
        /// Complete replacement call.
        evidence: Span,
    },
    /// A captured string slice with nonnegative, exact UTF-16 code-unit indices.
    /// Materialization refuses a nonempty result containing an unpaired surrogate.
    Slice {
        /// Captured primitive-string receiver.
        input: Box<Self>,
        /// Inclusive start, clamped to the receiver's UTF-16 length.
        start: usize,
        /// Exclusive end, clamped to that length; omission uses the full length.
        end: Option<usize>,
        /// Complete slice call.
        evidence: Span,
    },
    /// String-only addition, without object conversion or property access.
    Concat {
        /// Captured left primitive string.
        left: Box<Self>,
        /// Captured right primitive string.
        right: Box<Self>,
        /// Complete addition expression.
        evidence: Span,
    },
}

/// A frozen source dependency or an explicit forecast premise.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[non_exhaustive]
pub enum JavaScriptDependency {
    /// A lexical name's value before any later rebinding.
    Binding {
        /// Source identifier spelling.
        name: String,
        /// Source-local assignment ordinal starting at one.
        version: usize,
        /// Defining declaration or assignment statement.
        evidence: Span,
    },
    /// An immutable value returned by one modeled read.
    ReadValue {
        /// Source-local read identity.
        read_id: usize,
        /// Exact read call.
        evidence: Span,
    },
    /// An exclusively owned descriptor defined by a canonical opening call.
    Descriptor {
        /// Identity local to the source identified by `opened_at.source_id`.
        descriptor_id: usize,
        /// Opening call, retained after lexical names have been resolved.
        opened_at: Span,
    },
    /// Host-provided entry cwd, never inferred from the analyzer filesystem.
    EntryWorkingDirectory {
        /// Literal anchor, if supplied.
        cwd: Option<String>,
    },
    /// Source-local mutation order, without assuming distinct path identities.
    FilesystemEpoch {
        /// Number of preceding modeled writes.
        epoch: usize,
    },
    /// A named standard-runtime hypothesis.
    Capability {
        /// Conditional behavior used by this operation.
        assumption: JavaScriptAssumption,
    },
}

/// An opaque executing point that prevents dependent successor forecasts.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JavaScriptBarrier {
    /// Exact expression or statement establishing the uncertainty.
    pub evidence: Span,
    /// Finite diagnostic category.
    pub reason: JavaScriptBarrierReason,
}

/// Why the finite Node producer cannot cross an execution point.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum JavaScriptBarrierReason {
    /// A name is unavailable or its lexical declaration is uninitialized.
    UnknownName,
    /// Callable behavior is outside the canonical Node/string subset.
    UnmodeledCall,
    /// An otherwise valid expression has unsupported values or semantics.
    UnsupportedOperation,
}

/// Visible conditional premises for this fresh CommonJS forecast lane.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum JavaScriptAssumption {
    /// Ordinary CommonJS evaluation in a fresh lexical environment.
    StandardNodeCommonJsEvaluation,
    /// Standard `require` and unmodified canonical `fs` exports at entry.
    CanonicalNodeFs,
    /// Ordinary unmodified primitive-string addition, replacement and slicing.
    StandardStringMethods,
    /// No external namespace, cwd, environment or filesystem mutation.
    NoConcurrentMutation,
    /// Descriptor targets are regular files, whose byte offsets and truncation
    /// semantics apply; devices, pipes, and other special files are excluded.
    RegularDescriptorFiles,
    /// Each modeled `writeSync` writes every supplied UTF-8 byte. This is an
    /// explicit forecast premise, never inferred from source or normal return.
    CompleteDescriptorWrites,
}

pub(crate) fn extract(
    parser: &mut Parser,
    source: &str,
    source_id: SourceId,
    cwd: Option<&str>,
    budget: DescriptionBudget,
) -> Result<JavaScriptPlan, SourceDescriptionFallback> {
    let tree = crate::source_description::parse(parser, source, budget)?;
    let declarations = syntax::validate(tree.root_node(), source, budget)?;
    eval::extract(
        tree.root_node(),
        source,
        source_id,
        cwd,
        budget,
        declarations,
    )
}
