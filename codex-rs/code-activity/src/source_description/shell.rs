//! Closed shell composition and owned literal children; no effects inferred.

mod accounting;
mod atoms;
mod composition;
mod inputs;

#[path = "shell_literals.rs"]
mod literals;
#[path = "shell_redirections.rs"]
mod redirections;

#[cfg(test)]
#[path = "shell_numeric_tests.rs"]
mod numeric_tests;

#[cfg(test)]
#[path = "shell_redirect_ownership_tests.rs"]
mod redirect_ownership_tests;

use super::DescriptionBudget;
use super::MAX_REFERENCES;
use super::MAX_RETAINED_BYTES;
use super::SourceDescriptionFallback;
use super::SourceReference;
use super::SourceReferenceBasis;
use super::SourceReferenceKind;
use crate::Language;
use crate::Span;
use crate::shell::input;
use tree_sitter::Node;

type Result<T> = std::result::Result<T, SourceDescriptionFallback>;
const UNSUPPORTED: SourceDescriptionFallback = SourceDescriptionFallback::UnsupportedSource;
const MAX_EXTENSION_EVENTS: usize = 4096;

pub(super) struct ChildSource {
    pub(super) language: Language,
    pub(super) source: String,
    pub(super) evidence: Span,
}

pub(super) struct Plan {
    pub(super) references: Vec<SourceReference>,
    pub(super) children: Vec<ChildSource>,
    pub(super) extension_events: usize,
    pub(super) extension_name_bytes: usize,
}

pub(super) fn extract(root: Node<'_>, source: &str, budget: DescriptionBudget) -> Result<Plan> {
    if root.kind() != "program" || root.has_error() || source.contains('\0') {
        return Err(UNSUPPORTED);
    }
    let mut state = State {
        source,
        budget,
        plan: Plan {
            references: vec![],
            children: vec![],
            extension_events: 0,
            extension_name_bytes: 0,
        },
        retained: 0,
    };
    state.sequence(&literals::parts(root, budget)?)?;
    state.check_retained()?;
    budget.check()?;
    Ok(state.plan)
}

struct State<'source> {
    source: &'source str,
    budget: DescriptionBudget,
    plan: Plan,
    retained: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WordOrigin {
    InheritedLiteral,
    DecimalArgument,
}

struct Word {
    value: String,
    evidence: Span,
    origin: WordOrigin,
}

struct Assignment {
    name: String,
    spelling: String,
    evidence: Span,
}

struct Atom<'tree> {
    evidence: Span,
    words: Vec<Word>,
    assignments: Vec<Assignment>,
    redirects: Vec<redirections::FileRedirect>,
    heredoc: Option<redirections::Heredoc<'tree>>,
}

enum AtomInput {
    Ordinary,
    Directory { target: String, evidence: Span },
    Code(ChildSource),
    LiteralData { evidence: Span },
}

fn environment_name(value: &str) -> bool {
    let mut bytes = value.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z' | b'A'..=b'Z' | b'_'))
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn push_assignment(assignments: &mut Vec<Assignment>, assignment: Assignment) -> Result<()> {
    if assignments
        .iter()
        .any(|existing| existing.name == assignment.name)
    {
        return Err(UNSUPPORTED);
    }
    assignments.push(assignment);
    Ok(())
}
