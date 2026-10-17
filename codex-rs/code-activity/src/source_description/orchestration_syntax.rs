//! Bounded exact CST shapes and conservative ordinary-string decoding.

use super::DescriptionBudget;
use super::Result;
use super::SourceDescriptionFallback;
use super::UNSUPPORTED;
use tree_sitter::Node;

pub(super) fn field<'tree>(node: Node<'tree>, name: &str) -> Result<Node<'tree>> {
    node.child_by_field_name(name).ok_or(UNSUPPORTED)
}

pub(super) fn parts(node: Node<'_>, budget: DescriptionBudget) -> Result<Vec<Node<'_>>> {
    let mut result = Vec::new();
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        budget.check()?;
        if child.kind() == "comment" {
            continue;
        }
        if result.len() >= 256 {
            return Err(SourceDescriptionFallback::SourceLimit);
        }
        result.push(child);
    }
    Ok(result)
}

pub(super) fn punctuation(
    node: Node<'_>,
    allowed: &[&str],
    budget: DescriptionBudget,
) -> Result<()> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        budget.check()?;
        if !child.is_named() && !allowed.contains(&child.kind()) {
            return Err(UNSUPPORTED);
        }
    }
    Ok(())
}

pub(super) fn sequence<'tree>(
    node: Node<'tree>,
    open: &str,
    close: &str,
    budget: DescriptionBudget,
) -> Result<Vec<Node<'tree>>> {
    let mut cursor = node.walk();
    let mut children = node
        .children(&mut cursor)
        .filter(|child| child.kind() != "comment");
    if children.next().is_none_or(|child| child.kind() != open) {
        return Err(UNSUPPORTED);
    }
    let mut result = Vec::new();
    let mut expecting_value = true;
    let mut closed = false;
    for child in children {
        budget.check()?;
        if closed {
            return Err(UNSUPPORTED);
        }
        if !child.is_named() && child.kind() == close {
            closed = true;
        } else if expecting_value && child.is_named() {
            if result.len() >= 8 {
                return Err(SourceDescriptionFallback::SourceLimit);
            }
            result.push(child);
            expecting_value = false;
        } else if !expecting_value && child.kind() == "," {
            expecting_value = true;
        } else {
            return Err(UNSUPPORTED);
        }
    }
    if closed { Ok(result) } else { Err(UNSUPPORTED) }
}

pub(super) fn identifier<'source>(node: Node<'_>, source: &'source str) -> Result<&'source str> {
    let name = crate::text(node, source);
    if node.kind() == "identifier" && name.is_ascii() && !name.contains('\\') {
        Ok(name)
    } else {
        Err(UNSUPPORTED)
    }
}

pub(super) fn call(node: Node<'_>, budget: DescriptionBudget) -> Result<(Node<'_>, Node<'_>)> {
    if node.kind() != "call_expression" || parts(node, budget)?.len() != 2 {
        return Err(UNSUPPORTED);
    }
    punctuation(node, &[], budget)?;
    let function = field(node, "function")?;
    let arguments = field(node, "arguments")?;
    if arguments.kind() != "arguments" {
        return Err(UNSUPPORTED);
    }
    Ok((function, arguments))
}

pub(super) fn member<'source>(
    node: Node<'_>,
    source: &'source str,
    budget: DescriptionBudget,
) -> Result<(&'source str, &'source str)> {
    let (object, property) = member_parts(node, source, budget)?;
    Ok((identifier(object, source)?, property))
}

pub(super) fn member_parts<'tree, 'source>(
    node: Node<'tree>,
    source: &'source str,
    budget: DescriptionBudget,
) -> Result<(Node<'tree>, &'source str)> {
    if node.kind() != "member_expression" || parts(node, budget)?.len() != 2 {
        return Err(UNSUPPORTED);
    }
    punctuation(node, &["."], budget)?;
    let object = field(node, "object")?;
    let property = field(node, "property")?;
    if property.kind() != "property_identifier" {
        return Err(UNSUPPORTED);
    }
    Ok((object, crate::text(property, source)))
}

pub(super) fn decimal(node: Node<'_>, source: &str) -> Result<String> {
    decimal_value(node, source)?;
    Ok(crate::text(node, source).to_owned())
}

pub(super) fn decimal_value(node: Node<'_>, source: &str) -> Result<u64> {
    let raw = crate::text(node, source);
    if node.kind() != "number"
        || node.child_count() != 0
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
    Ok(value)
}

pub(super) fn signed_decimal(
    node: Node<'_>,
    source: &str,
    positive_max: u64,
    negative_max: u64,
    budget: DescriptionBudget,
) -> Result<String> {
    budget.check()?;
    if node.kind() == "number" {
        if decimal_value(node, source)? > positive_max {
            return Err(UNSUPPORTED);
        }
        return Ok(crate::text(node, source).to_owned());
    }
    if node.kind() != "unary_expression" || field(node, "operator")?.kind() != "-" {
        return Err(UNSUPPORTED);
    }
    punctuation(node, &["-"], budget)?;
    let children = parts(node, budget)?;
    let [argument] = children.as_slice() else {
        return Err(UNSUPPORTED);
    };
    if *argument != field(node, "argument")? {
        return Err(UNSUPPORTED);
    }
    let magnitude = decimal_value(*argument, source)?;
    if magnitude == 0 || magnitude > negative_max {
        return Err(UNSUPPORTED);
    }
    Ok(format!("-{}", crate::text(*argument, source)))
}

pub(super) fn parenthesized(node: Node<'_>, budget: DescriptionBudget) -> Result<Node<'_>> {
    if node.kind() != "parenthesized_expression" {
        return Err(UNSUPPORTED);
    }
    punctuation(node, &["(", ")"], budget)?;
    let children = parts(node, budget)?;
    let [child] = children.as_slice() else {
        return Err(UNSUPPORTED);
    };
    Ok(*child)
}

pub(super) fn record_identifier(raw: &str) -> bool {
    let mut bytes = raw.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || matches!(byte, b'_' | b'$'))
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$'))
}

/// Only direct lexical declarations share the outer JSON lexical scope.
pub(super) fn declares_json(
    statements: &[Node<'_>],
    source: &str,
    budget: DescriptionBudget,
) -> Result<bool> {
    for statement in statements {
        budget.check()?;
        if statement.kind() != "lexical_declaration" {
            continue;
        }
        for declaration in parts(*statement, budget)? {
            if declaration.kind() == "variable_declarator"
                && declaration.child_by_field_name("name").is_some_and(|name| {
                    name.kind() == "identifier" && crate::text(name, source) == "JSON"
                })
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

pub(super) fn decode(raw: &str, budget: DescriptionBudget) -> Result<String> {
    super::strings::quoted(raw, budget)
}

pub(super) fn reserved(name: &str) -> bool {
    matches!(
        name,
        "tools"
            | "Promise"
            | "text"
            | "await"
            | "yield"
            | "eval"
            | "arguments"
            | "break"
            | "case"
            | "catch"
            | "class"
            | "const"
            | "continue"
            | "debugger"
            | "default"
            | "delete"
            | "do"
            | "else"
            | "enum"
            | "export"
            | "extends"
            | "false"
            | "finally"
            | "for"
            | "function"
            | "if"
            | "implements"
            | "import"
            | "in"
            | "instanceof"
            | "interface"
            | "let"
            | "new"
            | "null"
            | "package"
            | "private"
            | "protected"
            | "public"
            | "return"
            | "static"
            | "super"
            | "switch"
            | "this"
            | "throw"
            | "true"
            | "try"
            | "typeof"
            | "var"
            | "void"
            | "while"
            | "with"
    )
}
