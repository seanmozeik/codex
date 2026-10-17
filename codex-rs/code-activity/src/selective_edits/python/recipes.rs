//! Captured text and index recipes.

use super::PythonReadNewline;
use super::TextEncoding;
use super::predicate::PredicateRecipe;
use crate::Span;
use crate::Target;
use serde::Serialize;

/// A bounded, immutable expression for the text supplied to a write.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum TextRecipe {
    /// Decoded source text, including its original token evidence.
    Literal {
        /// Decoded Unicode text.
        value: String,
        /// Original literal token, not the decoded text's byte coordinates.
        evidence: Span,
    },
    /// Text read at this program point; it is not a supplied file snapshot.
    Read {
        /// Stable zero-based read identity within evidence.source_id only.
        read_id: usize,
        /// Source path spelling and entry cwd.
        target: Target,
        /// Read method call evidence.
        evidence: Span,
        /// Source-local preceding mutations; not a cross-source file snapshot.
        filesystem_epoch: usize,
        /// Declared encoding or runtime default.
        encoding: TextEncoding,
        /// Explicit source decoding policy; absent retains legacy universal reads.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        newline: Option<PythonReadNewline>,
    },
    /// Standard string replacement, kept as a recipe rather than executed.
    Replace {
        /// Captured input expression.
        input: Box<TextRecipe>,
        /// Literal-origin search text.
        old: String,
        /// Literal-origin replacement text.
        new: String,
        /// Maximum replacements; absent means the standard unbounded form.
        count: Option<usize>,
        /// Full replacement call evidence.
        evidence: Span,
    },
    /// Replacement with captured, proven text expressions as arguments.
    ReplaceText {
        /// Captured string receiver.
        input: Box<TextRecipe>,
        /// Captured search expression.
        old: Box<TextRecipe>,
        /// Captured replacement expression.
        new: Box<TextRecipe>,
        /// Maximum replacements, absent for unlimited.
        count: Option<usize>,
        /// Full method invocation.
        evidence: Span,
    },
    /// Standard Python code-point slice, with unit positive step.
    Slice {
        /// Captured string receiver.
        input: Box<TextRecipe>,
        /// Inclusive lower bound, absent for Python's default.
        start: Option<Box<IndexRecipe>>,
        /// Exclusive upper bound, absent for Python's default.
        end: Option<Box<IndexRecipe>>,
        /// Complete subscription expression.
        evidence: Span,
    },
    /// One item of an explicit-separator split, conditional on a valid index.
    SplitItem {
        /// Captured string receiver.
        input: Box<TextRecipe>,
        /// Nonempty literal-origin separator.
        separator: String,
        /// Python maximum splits; negative and absent mean unlimited.
        maxsplit: Option<i64>,
        /// Python list index, including negative values.
        index: i64,
        /// Original split method invocation.
        split_evidence: Span,
        /// Complete item subscription.
        evidence: Span,
    },
    /// Remove trailing code points present in the supplied character set.
    Rstrip {
        /// Captured string receiver.
        input: Box<TextRecipe>,
        /// Literal-origin set of characters, not a suffix.
        characters: String,
        /// Complete method invocation.
        evidence: Span,
    },
    /// Left-to-right text concatenation; only literal leaves may form paths.
    Concat {
        /// Captured left text.
        left: Box<TextRecipe>,
        /// Captured right text.
        right: Box<TextRecipe>,
        /// Full addition expression evidence.
        evidence: Span,
    },
    /// A lazy choice between closed, immutable text expressions.
    Conditional {
        /// Captured standard text predicate, evaluated before either arm.
        predicate: Box<PredicateRecipe>,
        /// Evaluated only when the predicate is true.
        consequence: Box<TextRecipe>,
        /// Evaluated only when the predicate is false.
        alternative: Box<TextRecipe>,
        /// Complete conditional expression evidence.
        evidence: Span,
    },
}

/// A frozen Python integer used as a text slice/index-method bound.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum IndexRecipe {
    /// A signed integer whose value is known from source.
    Literal {
        /// Python integer value within the bounded signed representation.
        value: i64,
        /// Original integer expression or its argument-use occurrence.
        evidence: Span,
    },
    /// str.index: code-point position, conditional on finding the substring.
    SubstringIndex {
        /// Captured string receiver.
        input: Box<TextRecipe>,
        /// Literal-origin substring.
        needle: String,
        /// Python start bound, absent for zero.
        start: Option<Box<IndexRecipe>>,
        /// Full index method invocation.
        evidence: Span,
    },
}
