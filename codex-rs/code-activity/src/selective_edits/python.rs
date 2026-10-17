//! Conditional Python edit forecasts with explicit recipes and runtime barriers.

mod eval;
mod literal;
mod logical;
mod predicate;
mod recipes;
mod syntax;

pub use logical::PythonLogicalWrite;
pub use logical::PythonReadNewline;
pub use logical::PythonStreamId;
pub use logical::PythonStreamMode;
pub use logical::PythonStreamPosition;
pub use logical::PythonWriteNewline;
pub use logical::PythonWriteResultCondition;
pub use predicate::PredicateRecipe;
pub use recipes::IndexRecipe;
pub use recipes::TextRecipe;

use crate::SourceDescriptionFallback;
use crate::SourceId;
use crate::Span;
use crate::Target;
use crate::source_description::DescriptionBudget;
use serde::Serialize;
use tree_sitter::Parser;

/// Ordered predictions for one fully parsed Python source unit.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PythonPlan {
    /// Physical mutations and logical stream acceptances in evaluation order.
    pub edits: Vec<PythonEdit>,
    /// The first opaque executing operation, if one is encountered.
    pub barriers: Vec<PythonBarrier>,
    /// Conditional standard API hypotheses, never host runtime attestations.
    pub assumptions: Vec<PythonAssumption>,
    /// Modeled calls required to finish this source normally.
    pub completion_conditions: Vec<Span>,
}

pub(super) struct PythonExtraction {
    pub(super) plan: PythonPlan,
    pub(super) suffix_touches: Vec<PythonSuffixTouch>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PythonSuffixTouch {
    pub(super) edit_ordinal: usize,
    pub(super) write_evidence: Span,
    pub(super) stream_id: PythonStreamId,
    pub(super) opened_at: Span,
    pub(super) flush_evidence: Span,
    pub(super) truncate_evidence: Span,
}

/// One predicted mutation, without a claim that it ran or established final bytes.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PythonEdit {
    /// Zero-based effect order, separate from physical epochs and stream revisions.
    pub ordinal: usize,
    /// Enclosing literal loop iterations, outermost first; absent off-loop.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub iteration_context: Vec<PythonIterationContext>,
    /// Exact source operation bytes; implicit context close uses its enclosing with statement.
    pub evidence: Span,
    /// Literal spelling and entry cwd; no filesystem resolution occurs.
    pub target: Target,
    /// Mutation primitive and, for a write, its unevaluated text recipe.
    pub operation: PythonOperation,
    /// Frozen source dependencies used by this operation.
    pub dependencies: Vec<PythonDependency>,
    /// Earlier modeled calls whose normal completion is required.
    pub predecessor_conditions: Vec<Span>,
    /// Later source context; never an observed final-state guarantee.
    pub final_state: PythonFinalState,
}

/// One source-literal iteration enclosing a predicted operation.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PythonIterationContext {
    /// Full enclosing for statement, separate from exact operation evidence.
    pub evidence: Span,
    /// Zero-based iteration index in the source literal.
    pub index: usize,
    /// Number of entries in the source literal iterable.
    pub literal_count: usize,
}

/// A physical mutation or distinct logical acceptance under standard API premises.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PythonOperation {
    /// One conditional buffered stream acceptance, never an immediate file mutation.
    LogicalWrite(PythonLogicalWrite),
    /// A successful write-mode open creates or truncates the named file.
    Truncate,
    /// One replacing text write, after the recorded predecessors.
    WriteText {
        /// Operation-point text recipe; it contains no host filesystem bytes.
        content: TextRecipe,
        /// Declared encoding, or an explicitly unresolved runtime default.
        encoding: TextEncoding,
        /// Explicit source output policy; absent retains the runtime-default policy.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        newline: Option<PythonWriteNewline>,
    },
}

/// A dependency on a frozen source value or a named forecast capability.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PythonDependency {
    /// A local name version captured before any subsequent rebinding.
    Binding {
        /// Source identifier spelling.
        name: String,
        /// Global assignment/import/definition ordinal, starting at one.
        version: usize,
        /// Defining statement evidence.
        evidence: Span,
    },
    /// A captured text read that remains stable across later mutations.
    ReadValue {
        /// Identity of the read recipe within evidence.source_id only.
        read_id: usize,
        /// Evidence of that read.
        evidence: Span,
    },
    /// Working directory at entry, never resolved on the analyzer host.
    EntryWorkingDirectory {
        /// Host-supplied spelling, if available.
        cwd: Option<String>,
    },
    /// A conditional API hypothesis, not an authority or runtime profile.
    Capability {
        /// Named standard behavior used by the recipe.
        assumption: PythonAssumption,
    },
    /// Filesystem mutation order; distinct spellings may refer to one object.
    FilesystemEpoch {
        /// Number of preceding modeled mutations.
        epoch: usize,
    },
}

/// One opaque execution point that inhibits dependent later forecasts.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PythonBarrier {
    /// Exact source expression or statement requiring an unknown effect summary.
    pub evidence: Span,
    /// Finite explanation; no unchecked diagnostic source is executed.
    pub reason: PythonBarrierReason,
}

/// Why an executing expression cannot be safely crossed by this producer.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PythonBarrierReason {
    /// Name lookup lacks a modeled source binding or explicit capability.
    UnknownName,
    /// Callable or method effects are outside the finite registry.
    UnmodeledCall,
    /// Import initialization is outside the canonical pathlib forecast.
    UnmodeledImport,
    /// A recognized expression has unsupported values or semantics.
    UnsupportedOperation,
    /// A live file handle escapes the direct one-use expression registry.
    EscapingHandle,
}

/// Final-state uncertainty associated with later source operations.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PythonFinalState {
    /// Source interpretation establishes no observed final file state.
    NotEstablished,
    /// A later modeled mutation uses exactly the same path/cwd spelling.
    KnownLaterTouch,
    /// A later different spelling may alias the same filesystem object.
    PossibleAliasLaterTouch,
    /// A later opaque execution point may change or replace this file.
    OpaqueSuccessor,
}

/// Text encoding used by the modeled call.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TextEncoding {
    /// Explicit UTF-8 or its accepted utf8 alias.
    Utf8,
    /// Encoding omitted by source; the runtime choice is not inferred.
    RuntimeDefault,
}

/// Visible conditional hypotheses for the standard API forecast lane.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PythonAssumption {
    /// Ordinary Python evaluation order and standard primitive behavior.
    StandardPythonEvaluation,
    /// The initially available builtin open has standard Python behavior.
    StandardBuiltinOpen,
    /// Modeled pathlib imports yield ordinary pathlib module and Path objects.
    CanonicalPathlib,
    /// The runtime pathlib text API supports its source-declared newline argument.
    PathlibNewlineSupport,
    /// Ordinary immutable text operations, with explicit exception conditions.
    StandardTextMethods,
    /// No external concurrent namespace, cwd, environment, or file mutation.
    NoConcurrentMutation,
}

pub(super) fn extract(
    parser: &mut Parser,
    source: &str,
    source_id: SourceId,
    cwd: Option<&str>,
    endpoint: super::PythonWriteEndpoint,
    budget: DescriptionBudget,
) -> Result<PythonExtraction, SourceDescriptionFallback> {
    if source.len() > crate::GUARD_MAX_SOURCE_BYTES {
        return Err(SourceDescriptionFallback::SourceLimit);
    }
    let tree = crate::source_description::parse(parser, source, budget)?;
    syntax::validate(tree.root_node(), source, budget)?;
    eval::extract(tree.root_node(), source, source_id, cwd, endpoint, budget)
}
