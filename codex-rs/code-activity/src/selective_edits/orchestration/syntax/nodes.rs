//! Checked CST shapes shared by syntax validation and carrier reading.

use super::super::MAX_ITEMS;
use super::super::Result;
use super::super::UNSUPPORTED;
use super::Syntax;
use super::lexical::reserved;
use crate::source_description::SourceDescriptionFallback;
use tree_sitter::Node;

impl Syntax<'_> {
    pub(in super::super) fn parts<'tree>(&self, node: Node<'tree>) -> Result<Vec<Node<'tree>>> {
        let mut result = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.budget.check()?;
            if child.kind() != "comment" {
                if result.len() >= MAX_ITEMS || child.is_missing() || child.has_error() {
                    return Err(if result.len() >= MAX_ITEMS {
                        SourceDescriptionFallback::SourceLimit
                    } else {
                        UNSUPPORTED
                    });
                }
                result.push(child);
            }
        }
        Ok(result)
    }

    pub(super) fn punctuation(&self, node: Node<'_>, allowed: &[&str]) -> Result<()> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.budget.check()?;
            if child.is_missing() || (!child.is_named() && !allowed.contains(&child.kind())) {
                return Err(UNSUPPORTED);
            }
        }
        Ok(())
    }

    pub(in super::super) fn sequence<'tree>(
        &self,
        node: Node<'tree>,
        open: &str,
        close: &str,
    ) -> Result<Vec<Node<'tree>>> {
        let mut cursor = node.walk();
        let mut children = node
            .children(&mut cursor)
            .filter(|child| child.kind() != "comment");
        if children
            .next()
            .is_none_or(|child| child.is_named() || child.kind() != open)
        {
            return Err(UNSUPPORTED);
        }
        let mut result = Vec::new();
        let mut expecting_value = true;
        let mut closed = false;
        for child in children {
            self.budget.check()?;
            if closed || child.is_missing() {
                return Err(UNSUPPORTED);
            }
            if !child.is_named() && child.kind() == close {
                closed = true;
            } else if expecting_value && child.is_named() {
                if result.len() >= MAX_ITEMS {
                    return Err(SourceDescriptionFallback::SourceLimit);
                }
                result.push(child);
                expecting_value = false;
            } else if !expecting_value && !child.is_named() && child.kind() == "," {
                expecting_value = true;
            } else {
                return Err(UNSUPPORTED);
            }
        }
        if closed { Ok(result) } else { Err(UNSUPPORTED) }
    }

    pub(in super::super) fn identifier<'a>(&'a self, node: Node<'_>) -> Result<&'a str> {
        let name = crate::text(node, self.source);
        let mut bytes = name.bytes();
        if !matches!(node.kind(), "identifier" | "shorthand_property_identifier")
            || node.child_count() != 0
            || !bytes
                .next()
                .is_some_and(|b| b.is_ascii_alphabetic() || matches!(b, b'_' | b'$'))
            || !bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'$'))
            || reserved(name)
        {
            return Err(UNSUPPORTED);
        }
        Ok(name)
    }

    pub(in super::super) fn parenthesized<'tree>(&self, node: Node<'tree>) -> Result<Node<'tree>> {
        self.punctuation(node, &["(", ")"])?;
        let children = self.parts(node)?;
        let [value] = children.as_slice() else {
            return Err(UNSUPPORTED);
        };
        Ok(*value)
    }

    pub(in super::super) fn peel<'tree>(&self, mut node: Node<'tree>) -> Result<Node<'tree>> {
        while node.kind() == "parenthesized_expression" {
            self.budget.check()?;
            node = self.parenthesized(node)?;
        }
        Ok(node)
    }

    pub(in super::super) fn member<'tree>(&self, node: Node<'tree>) -> Result<(Node<'tree>, &str)> {
        self.punctuation(node, &["."])?;
        let children = self.parts(node)?;
        let [object, property] = children.as_slice() else {
            return Err(UNSUPPORTED);
        };
        if node.child_by_field_name("object") != Some(*object)
            || node.child_by_field_name("property") != Some(*property)
            || property.kind() != "property_identifier"
            || property.child_count() != 0
        {
            return Err(UNSUPPORTED);
        }
        let property = crate::text(*property, self.source);
        if !property.is_ascii() || property.contains('\\') {
            return Err(UNSUPPORTED);
        }
        Ok((*object, property))
    }

    pub(in super::super) fn call<'tree>(
        &self,
        node: Node<'tree>,
    ) -> Result<(Node<'tree>, Vec<Node<'tree>>)> {
        self.punctuation(node, &[])?;
        let children = self.parts(node)?;
        let [callee, arguments] = children.as_slice() else {
            return Err(UNSUPPORTED);
        };
        if node.child_by_field_name("function") != Some(*callee)
            || node.child_by_field_name("arguments") != Some(*arguments)
            || arguments.kind() != "arguments"
        {
            return Err(UNSUPPORTED);
        }
        Ok((*callee, self.sequence(*arguments, "(", ")")?))
    }

    pub(in super::super) fn pair<'tree>(&self, node: Node<'tree>) -> Result<(String, Node<'tree>)> {
        if node.kind() != "pair" {
            return Err(UNSUPPORTED);
        }
        self.punctuation(node, &[":"])?;
        let children = self.parts(node)?;
        let [key, value] = children.as_slice() else {
            return Err(UNSUPPORTED);
        };
        if node.child_by_field_name("key") != Some(*key)
            || node.child_by_field_name("value") != Some(*value)
        {
            return Err(UNSUPPORTED);
        }
        let name = match key.kind() {
            "property_identifier" if key.child_count() == 0 => {
                let raw = crate::text(*key, self.source);
                if !raw.is_ascii() || raw.contains('\\') {
                    return Err(UNSUPPORTED);
                }
                raw.to_owned()
            }
            "string" => self.literal_text(*key)?,
            _ => return Err(UNSUPPORTED),
        };
        Ok((name, *value))
    }
}
