//! Conservative strict-only guards for semantics the diagnostic baseline skips.

use crate::Language;
use crate::SourceId;
use crate::Unresolved;
use crate::whole_script::AnalysisBudget;
use tree_sitter::Node;

pub(crate) fn check(
    root: Node<'_>,
    source: &str,
    source_id: SourceId,
    language: Language,
    budget: AnalysisBudget,
) -> Option<Unresolved> {
    let mut cursor = root.walk();
    loop {
        let node = cursor.node();
        let reason = if budget.expired() {
            Some("Whole-script analysis time budget exceeded")
        } else {
            match language {
                Language::Python => python_gap(node),
                Language::TypeScript => typescript_gap(node, source),
                Language::Shell => None,
            }
        };
        if let Some(reason) = reason {
            return Some(Unresolved {
                evidence: crate::span(node, source_id),
                reason: reason.into(),
            });
        }
        if cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return None;
            }
        }
    }
}

fn typescript_gap(node: Node<'_>, source: &str) -> Option<&'static str> {
    match node.kind() {
        "identifier" | "shorthand_property_identifier" if crate::text(node, source) == "Deno" => {
            Some("Deno runtime semantics are outside the initial strict JavaScript scope")
        }
        "assignment_expression" | "augmented_assignment_expression"
            if node
                .child_by_field_name("left")
                .is_none_or(|left| left.kind() != "identifier") =>
        {
            Some("JavaScript assignment target evaluation and mutation are not modeled")
        }
        "update_expression" => {
            Some("JavaScript update target evaluation and mutation are not modeled")
        }
        _ => None,
    }
}

fn python_gap(node: Node<'_>) -> Option<&'static str> {
    match node.kind() {
        "call" if many_python_positional_arguments(node) => Some(
            "Python calls with more than two positional arguments may hide unmodeled options or callbacks",
        ),
        "assert_statement" => {
            Some("Python assertion evaluation and optional message are not modeled")
        }
        "comparison_operator" => Some("Python comparison short-circuit evaluation is not modeled"),
        "assignment" if node.child_by_field_name("type").is_some() => {
            Some("Python annotation evaluation depends on runtime version and annotation settings")
        }
        "function_definition" if node.child_by_field_name("return_type").is_some() => {
            Some("Python annotation evaluation depends on runtime version and annotation settings")
        }
        _ => None,
    }
}

fn many_python_positional_arguments(node: Node<'_>) -> bool {
    let Some(arguments) = node.child_by_field_name("arguments") else {
        return false;
    };
    let mut cursor = arguments.walk();
    arguments
        .named_children(&mut cursor)
        .filter(|argument| argument.kind() != "keyword_argument")
        .take(3)
        .count()
        > 2
}
