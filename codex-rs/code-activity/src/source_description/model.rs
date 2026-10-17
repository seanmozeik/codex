//! Source references cannot be converted into runtime effects or file changes.

use super::SourcePatch;
use crate::CallId;
use crate::Source;
use crate::Span;
use serde::Serialize;

/// Evidence is syntactic even when a method name resembles a filesystem API.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SourceReferenceBasis {
    /// A supported construct in the supplied source; not a runtime prediction.
    SourceSyntax,
}

/// Recognized source spelling, never an executed operation.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum SourceReferenceKind {
    /// A supported explicit module-binding declaration or require spelling.
    ImportFrom,
    /// A call through the declared Path constructor name.
    PathConstructorCall,
    /// A call spelled open with supported literal arguments.
    OpenCall,
    /// A supported read/read_text member call.
    ReadCall,
    /// A supported replace member call.
    ReplaceCall,
    /// A supported text/list/index transform spelling, not a computed value.
    TransformCall,
    /// A supported write/write_text member call.
    WriteCall,
    /// A recognized tool or literal shell invocation; no callable identity is asserted.
    InvocationCall,
    /// A supported literal command option, not an effective runtime setting.
    CommandOption,
    /// A supported output-consumer spelling, not observed output or completion.
    OutputConsumer,
    /// A literal shell connector or group delimiter, not observed control flow.
    ShellConnector,
    /// A literal redirection operand, not a resolved file or successful redirect.
    ShellRedirection,
    /// A literal cd operand, without resolving the runtime working directory.
    DirectoryOperand,
    /// A closed environment assignment spelling, not an effective runtime value.
    EnvironmentAssignment,
    /// Quoted heredoc data whose bytes are not interpreted as a child program.
    HeredocData,
}

/// One reference in a fully recognized source grammar.
/// Literal strings retain source spelling after decoding, without path resolution.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct SourceReference {
    /// Always source syntax, never observed or predicted effects.
    pub basis: SourceReferenceBasis,
    /// Source construct being described.
    pub kind: SourceReferenceKind,
    /// Call/declaration location in the associated source table.
    pub evidence: Span,
    /// Associated literal path or import name, when this form has one.
    pub literal: Option<String>,
}

/// Why the entire source description is absent.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum SourceDescriptionFallback {
    /// Any unsupported construct or unresolved syntactic origin.
    UnsupportedSource,
    /// Cooperative elapsed-time budget expired.
    TimeBudget,
    /// Source, node, depth, binding, reference or retained-byte bound exceeded.
    SourceLimit,
    /// This language has no complete source-description grammar.
    UnsupportedLanguage,
}

/// Completeness of supported syntax, never runtime-effect coverage.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum SourceDescriptionDisposition {
    /// Every non-comment construct was recognized by the closed grammar.
    CompleteSyntax,
    /// No source references may be displayed.
    Fallback(SourceDescriptionFallback),
}

/// Atomic source-only description. Every fallback has no enrichment or source text.
/// This type has no effects, diffs, runtime profile or execution-status fields.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct SourceDescription {
    call_id: CallId,
    disposition: SourceDescriptionDisposition,
    sources: Vec<Source>,
    #[serde(skip)]
    source_texts: Vec<String>,
    references: Vec<SourceReference>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    patches: Vec<SourcePatch>,
}

impl SourceDescription {
    /// Original opaque call identity.
    #[must_use]
    pub const fn call_id(&self) -> &CallId {
        &self.call_id
    }

    /// Complete grammar recognition or whole-source fallback.
    #[must_use]
    pub const fn disposition(&self) -> SourceDescriptionDisposition {
        self.disposition
    }

    /// Source-only references; always empty on fallback.
    #[must_use]
    pub fn references(&self) -> &[SourceReference] {
        &self.references
    }

    /// Source locations for reference evidence, never runtime provenance.
    #[must_use]
    pub fn sources(&self) -> &[Source] {
        &self.sources
    }

    /// Exact source text, indexed by the matching source ID, for evidence checks.
    /// This complete-only collection shares the aggregate decoded-source limit
    /// and is never included in this type's serialized representation.
    #[must_use]
    pub fn source_texts(&self) -> &[String] {
        &self.source_texts
    }

    /// Completely parsed requested patch text; never observed file changes.
    /// Unsupported siblings discard these records with all other enrichment.
    #[must_use]
    pub fn patches(&self) -> &[SourcePatch] {
        &self.patches
    }

    pub(crate) fn complete(
        call_id: CallId,
        sources: Vec<Source>,
        source_texts: Vec<String>,
        references: Vec<SourceReference>,
        patches: Vec<SourcePatch>,
    ) -> Self {
        Self {
            call_id,
            disposition: SourceDescriptionDisposition::CompleteSyntax,
            sources,
            source_texts,
            references,
            patches,
        }
    }

    pub(crate) fn fallback(call_id: CallId, reason: SourceDescriptionFallback) -> Self {
        Self {
            call_id,
            disposition: SourceDescriptionDisposition::Fallback(reason),
            sources: vec![],
            source_texts: vec![],
            references: vec![],
            patches: vec![],
        }
    }
}
