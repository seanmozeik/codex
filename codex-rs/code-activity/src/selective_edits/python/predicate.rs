//! Captured predicates with finite, side-effect-free standard text semantics.

use super::TextRecipe;
use crate::Span;
use serde::Serialize;

/// A standard Python text predicate over an immutable captured receiver.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PredicateRecipe {
    /// Python text membership. Evaluate the needle before the haystack.
    TextContains {
        /// Captured left text operand.
        needle: Box<TextRecipe>,
        /// Captured right text operand.
        haystack: Box<TextRecipe>,
        /// Complete comparison expression.
        evidence: Span,
    },
    /// Equality of two immutable text values, without coercion.
    TextEquals {
        /// Captured left text operand, evaluated first.
        left: Box<TextRecipe>,
        /// Captured right text operand.
        right: Box<TextRecipe>,
        /// Complete comparison expression.
        evidence: Span,
    },
    /// Boolean conjunction; evaluate the right predicate only if left is true.
    And {
        /// Closed boolean predicate evaluated first.
        left: Box<PredicateRecipe>,
        /// Closed boolean predicate evaluated conditionally.
        right: Box<PredicateRecipe>,
        /// Complete conjunction expression.
        evidence: Span,
    },
    /// Whether the captured text begins with the complete literal prefix.
    StartsWith {
        /// Captured receiver, including any earlier read identity.
        input: Box<TextRecipe>,
        /// Literal-origin prefix; tuples and start/end bounds are unsupported.
        prefix: String,
        /// Exact argument use, including an immutable literal alias occurrence.
        prefix_evidence: Span,
        /// Full predicate call evidence.
        evidence: Span,
    },
    /// Whether the captured text ends with the complete literal suffix.
    EndsWith {
        /// Captured receiver, including any earlier read identity.
        input: Box<TextRecipe>,
        /// Literal-origin suffix; tuples and start/end bounds are unsupported.
        suffix: String,
        /// Exact argument use, including an immutable literal alias occurrence.
        suffix_evidence: Span,
        /// Full predicate call evidence.
        evidence: Span,
    },
}
