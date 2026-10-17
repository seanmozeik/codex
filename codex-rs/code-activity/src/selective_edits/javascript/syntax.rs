//! Finite whole-source CommonJS checks before any prefix is published.

use super::declarations;
use crate::Language;
use crate::SourceDescriptionFallback as Fallback;
use crate::source_description::DescriptionBudget;
use std::collections::BTreeMap;
use tree_sitter::Node;

type Result<T> = std::result::Result<T, Fallback>;
const UNSUPPORTED: Fallback = Fallback::UnsupportedSource;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Mutability {
    Const,
    Let,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum BinaryOperator {
    Add,
    And,
    Or,
}

pub(super) fn validate(
    root: Node<'_>,
    source: &str,
    budget: DescriptionBudget,
) -> Result<BTreeMap<String, Mutability>> {
    if root.kind() != "program" {
        return Err(UNSUPPORTED);
    }
    lexical_gaps(root, source, budget)?;
    punctuation(root, source, &[])?;
    let mut declarations = BTreeMap::new();
    for statement in parts(root) {
        budget.check()?;
        match statement.kind() {
            "empty_statement" => punctuation(statement, source, &[";"])?,
            "lexical_declaration" => {
                let declaration = declarations::parse(statement, source)?;
                for name in declaration.target.names() {
                    if matches!(
                        name,
                        "require"
                            | "module"
                            | "exports"
                            | "__filename"
                            | "__dirname"
                            | "eval"
                            | "arguments"
                    ) || declarations
                        .insert(name.to_owned(), declaration.mutable)
                        .is_some()
                    {
                        return Err(UNSUPPORTED);
                    }
                    if declarations.len() > 256 {
                        return Err(Fallback::SourceLimit);
                    }
                }
                expression(declaration.initializer, source, budget)?;
            }
            "expression_statement" => {
                punctuation(statement, source, &[";"])?;
                let value = only(statement)?;
                if value.kind() == "assignment_expression" {
                    punctuation(value, source, &["="])?;
                    let (left, right) = pair(value, "left", "right")?;
                    let name = identifier(left, source)?;
                    if matches!(name, "eval" | "arguments") {
                        return Err(UNSUPPORTED);
                    }
                    expression(right, source, budget)?;
                } else {
                    expression(value, source, budget)?;
                }
            }
            _ => return Err(UNSUPPORTED),
        }
    }
    budget.check()?;
    Ok(declarations)
}

fn expression(node: Node<'_>, source: &str, budget: DescriptionBudget) -> Result<()> {
    budget.check()?;
    match node.kind() {
        "identifier" => {
            identifier(node, source)?;
        }
        "string" => {
            literal(node, source)?;
        }
        "number" => {
            integer(node, source)?;
        }
        "null" if crate::text(node, source) == "null" => {}
        "true" | "false" if crate::text(node, source) == node.kind() => {}
        "parenthesized_expression" => {
            punctuation(node, source, &["(", ")"])?;
            expression(only(node)?, source, budget)?;
        }
        "binary_expression" => {
            binary_operator(node, source)?;
            let (left, right) = pair(node, "left", "right")?;
            expression(left, source, budget)?;
            expression(right, source, budget)?;
        }
        "member_expression" => {
            let (object, _) = member(node, source)?;
            expression(object, source, budget)?;
        }
        "call_expression" => {
            let (callee, arguments) = call(node, source)?;
            expression(callee, source, budget)?;
            for argument in arguments {
                expression(argument, source, budget)?;
            }
        }
        _ => return Err(UNSUPPORTED),
    }
    Ok(())
}

pub(super) fn binary_operator(node: Node<'_>, source: &str) -> Result<BinaryOperator> {
    punctuation(node, source, &["+", "&&", "||"])?;
    match crate::text(field(node, "operator")?, source) {
        "+" => Ok(BinaryOperator::Add),
        "&&" => Ok(BinaryOperator::And),
        "||" => Ok(BinaryOperator::Or),
        _ => Err(UNSUPPORTED),
    }
}

pub(super) fn integer(node: Node<'_>, source: &str) -> Result<usize> {
    let raw = crate::text(node, source);
    if node.kind() != "number"
        || raw.is_empty()
        || !raw.bytes().all(|byte| byte.is_ascii_digit())
        || (raw.len() > 1 && raw.starts_with('0'))
    {
        return Err(UNSUPPORTED);
    }
    let value = raw.parse::<u64>().map_err(|_| UNSUPPORTED)?;
    if value > 9_007_199_254_740_991 {
        return Err(UNSUPPORTED);
    }
    usize::try_from(value).map_err(|_| UNSUPPORTED)
}

pub(super) fn call<'tree>(
    node: Node<'tree>,
    source: &str,
) -> Result<(Node<'tree>, Vec<Node<'tree>>)> {
    punctuation(node, source, &[])?;
    let (function, arguments) = pair(node, "function", "arguments")?;
    if arguments.kind() != "arguments" {
        return Err(UNSUPPORTED);
    }
    let mut cursor = arguments.walk();
    let mut children = arguments
        .children(&mut cursor)
        .filter(|child| child.kind() != "comment");
    if children.next().is_none_or(|child| child.kind() != "(") {
        return Err(UNSUPPORTED);
    }
    let mut result = Vec::new();
    let mut expects_value = true;
    let mut closed = false;
    for child in children {
        if closed || child.is_missing() {
            return Err(UNSUPPORTED);
        }
        if !child.is_named() && child.kind() == ")" {
            closed = true;
        } else if expects_value && child.is_named() {
            result.push(child);
            expects_value = false;
        } else if !expects_value && !child.is_named() && child.kind() == "," {
            expects_value = true;
        } else {
            return Err(UNSUPPORTED);
        }
    }
    if closed {
        Ok((function, result))
    } else {
        Err(UNSUPPORTED)
    }
}

pub(super) fn member<'tree, 'source>(
    node: Node<'tree>,
    source: &'source str,
) -> Result<(Node<'tree>, &'source str)> {
    punctuation(node, source, &["."])?;
    let (object, property) = pair(node, "object", "property")?;
    let name = crate::text(property, source);
    if property.kind() != "property_identifier" || !ascii_name(name) {
        return Err(UNSUPPORTED);
    }
    Ok((object, name))
}

pub(super) fn literal(node: Node<'_>, source: &str) -> Result<String> {
    let raw = crate::text(node, source);
    if node.kind() != "string" || raw.contains(['\r', '\n']) {
        return Err(UNSUPPORTED);
    }
    if raw.len() > 16_392 {
        return Err(Fallback::SourceLimit);
    }
    let value = crate::value::string_literal(raw, Language::TypeScript).ok_or(UNSUPPORTED)?;
    if value.len() > 16_384 {
        return Err(Fallback::SourceLimit);
    }
    Ok(value)
}

fn identifier<'source>(node: Node<'_>, source: &'source str) -> Result<&'source str> {
    identifier_kind(node, source, "identifier")
}

pub(super) fn identifier_kind<'source>(
    node: Node<'_>,
    source: &'source str,
    kind: &str,
) -> Result<&'source str> {
    let name = crate::text(node, source);
    if node.kind() != kind || !ascii_name(name) || reserved(name) {
        return Err(UNSUPPORTED);
    }
    Ok(name)
}

fn ascii_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|b| b.is_ascii_alphabetic() || matches!(b, b'_' | b'$'))
        && bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'$'))
}

fn reserved(name: &str) -> bool {
    const WORDS: &str = "await yield implements interface package private protected public static let enum break case catch class const continue debugger default delete do else export extends finally for function if import in instanceof new return super switch this throw try typeof var void while with null true false";
    WORDS.split_ascii_whitespace().any(|word| word == name)
}

pub(super) fn field<'tree>(node: Node<'tree>, name: &str) -> Result<Node<'tree>> {
    node.child_by_field_name(name).ok_or(UNSUPPORTED)
}

pub(super) fn only(node: Node<'_>) -> Result<Node<'_>> {
    let nodes = parts(node);
    let [value] = nodes.as_slice() else {
        return Err(UNSUPPORTED);
    };
    Ok(*value)
}

pub(super) fn pair<'tree>(
    node: Node<'tree>,
    left: &str,
    right: &str,
) -> Result<(Node<'tree>, Node<'tree>)> {
    let values = parts(node);
    let [first, second] = values.as_slice() else {
        return Err(UNSUPPORTED);
    };
    if field(node, left)? != *first || field(node, right)? != *second {
        return Err(UNSUPPORTED);
    }
    Ok((*first, *second))
}

pub(super) fn parts(node: Node<'_>) -> Vec<Node<'_>> {
    crate::children(node)
        .into_iter()
        .filter(|child| child.kind() != "comment")
        .collect()
}

pub(super) fn punctuation(node: Node<'_>, source: &str, allowed: &[&str]) -> Result<()> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.is_missing()
            || (!child.is_named()
                && (!allowed.contains(&child.kind()) || crate::text(child, source) != child.kind()))
        {
            return Err(UNSUPPORTED);
        }
    }
    Ok(())
}

fn lexical_gaps(root: Node<'_>, source: &str, budget: DescriptionBudget) -> Result<()> {
    let whitespace = |text: &str| {
        text.bytes()
            .all(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
    };
    let mut cursor = root.walk();
    let mut end = 0;
    if root.child_count() == 0 {
        return if whitespace(source) {
            Ok(())
        } else {
            Err(UNSUPPORTED)
        };
    }
    loop {
        budget.check()?;
        let node = cursor.node();
        if !matches!(node.kind(), "string" | "comment") && cursor.goto_first_child() {
            continue;
        }
        if node.is_missing() || !whitespace(&source[end..node.start_byte()]) {
            return Err(UNSUPPORTED);
        }
        end = node.end_byte();
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return if whitespace(&source[end..]) {
                    Ok(())
                } else {
                    Err(UNSUPPORTED)
                };
            }
        }
    }
}
