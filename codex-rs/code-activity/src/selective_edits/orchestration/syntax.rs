//! Whole-module and statement compatibility, independent of host trust.

mod expressions;
mod lexical;
mod nodes;

use super::Result;
use super::UNSUPPORTED;
use crate::source_description::DescriptionBudget;
use std::collections::BTreeSet;
use tree_sitter::Node;

pub(super) struct Syntax<'source> {
    pub(super) source: &'source str,
    pub(super) budget: DescriptionBudget,
}

impl Syntax<'_> {
    /// Every admitted statement is compatible with strict module parsing.
    /// Unknown runtime expressions are still recursively checked here; they
    /// are not a license to overlook TypeScript or module early errors.
    pub(super) fn module<'tree>(&self, root: Node<'tree>) -> Result<Vec<Node<'tree>>> {
        if root.kind() != "program" {
            return Err(UNSUPPORTED);
        }
        // The TypeScript grammar treats some non-ECMAScript characters as
        // invisible extras. A successful CST alone cannot certify these gaps.
        self.module_whitespace(0, root.start_byte())?;
        self.lexical_gaps(root)?;
        self.module_whitespace(root.end_byte(), self.source.len())?;
        let statements = self.parts(root)?;
        self.punctuation(root, &[])?;
        self.statement_list(&statements, true)?;
        Ok(statements)
    }

    fn statement_list(&self, statements: &[Node<'_>], module: bool) -> Result<()> {
        let mut names = BTreeSet::new();
        for statement in statements {
            self.budget.check()?;
            if statement.kind() == "lexical_declaration" {
                let (name, value, _) = self.declaration(*statement)?;
                if matches!(name, "eval" | "arguments")
                    || (module && matches!(name, "tools" | "text"))
                    || !names.insert(name.to_owned())
                {
                    return Err(UNSUPPORTED);
                }
                self.expression(value)?;
            } else {
                self.syntax_statement(*statement)?;
            }
        }
        Ok(())
    }

    // Statement bodies deliberately exclude lexical declarations: an unbraced
    // if/for body cannot contain let/const. Blocks introduce a fresh lexical set.
    // var/functions/classes remain outside the grammar, so no hidden hoist can
    // invalidate an earlier host binding or collide across nested scopes.
    fn syntax_statement(&self, node: Node<'_>) -> Result<()> {
        self.budget.check()?;
        match node.kind() {
            "empty_statement" => self.punctuation(node, &[";"]),
            "expression_statement" => self.expression(self.statement_expression(node)?),
            "statement_block" => {
                self.punctuation(node, &["{", "}"])?;
                self.statement_list(&self.parts(node)?, false)
            }
            "for_statement" => {
                self.punctuation(node, &["for", "(", ";", ")"])?;
                let parts = self.parts(node)?;
                let [initializer, condition, increment, body] = parts.as_slice() else {
                    return Err(UNSUPPORTED);
                };
                if node.child_by_field_name("initializer") != Some(*initializer)
                    || node.child_by_field_name("condition") != Some(*condition)
                    || node.child_by_field_name("increment") != Some(*increment)
                    || node.child_by_field_name("body") != Some(*body)
                    || initializer.kind() != "lexical_declaration"
                    || increment.kind() != "update_expression"
                {
                    return Err(UNSUPPORTED);
                }
                let (name, value, _) = self.declaration(*initializer)?;
                if matches!(name, "eval" | "arguments") {
                    return Err(UNSUPPORTED);
                }
                self.for_separators(*initializer, *condition, *increment)?;
                self.expression(value)?;
                // The condition field contains the expression and a separate
                // unnamed semicolon in the pinned TS grammar. Empty conditions
                // and sequence expressions are outside this finite subset.
                self.expression(*condition)?;
                self.expression(*increment)?;
                self.syntax_statement(*body)
            }
            "if_statement" => {
                self.punctuation(node, &["if"])?;
                let parts = self.parts(node)?;
                if !(2..=3).contains(&parts.len()) {
                    return Err(UNSUPPORTED);
                }
                if node.child_by_field_name("condition") != Some(parts[0])
                    || node.child_by_field_name("consequence") != Some(parts[1])
                    || parts[0].kind() != "parenthesized_expression"
                {
                    return Err(UNSUPPORTED);
                }
                self.expression(parts[0])?;
                self.syntax_statement(parts[1])?;
                if let Some(alternative) = parts.get(2) {
                    if node.child_by_field_name("alternative") != Some(*alternative)
                        || alternative.kind() != "else_clause"
                    {
                        return Err(UNSUPPORTED);
                    }
                    self.punctuation(*alternative, &["else"])?;
                    let children = self.parts(*alternative)?;
                    let [body] = children.as_slice() else {
                        return Err(UNSUPPORTED);
                    };
                    self.syntax_statement(*body)?;
                }
                Ok(())
            }
            _ => Err(UNSUPPORTED),
        }
    }

    fn for_separators(
        &self,
        initializer: Node<'_>,
        condition: Node<'_>,
        increment: Node<'_>,
    ) -> Result<()> {
        // A lexical declaration may use ASI elsewhere, but a for header cannot.
        let mut cursor = initializer.walk();
        let first = initializer
            .children(&mut cursor)
            .filter(|child| child.kind() != "comment")
            .last()
            .ok_or(UNSUPPORTED)?;
        let mut second = condition.next_sibling();
        while let Some(comment) = second.filter(|child| child.kind() == "comment") {
            self.budget.check()?;
            second = comment.next_sibling();
        }
        let second = second.ok_or(UNSUPPORTED)?;
        for separator in [first, second] {
            self.budget.check()?;
            if separator.kind() != ";"
                || separator.is_named()
                || separator.is_missing()
                || crate::text(separator, self.source) != ";"
            {
                return Err(UNSUPPORTED);
            }
        }
        if second.end_byte() > increment.start_byte() {
            return Err(UNSUPPORTED);
        }
        Ok(())
    }

    pub(super) fn declaration<'tree>(
        &self,
        node: Node<'tree>,
    ) -> Result<(&str, Node<'tree>, bool)> {
        self.punctuation(node, &["const", "let", ";"])?;
        let parts = self.parts(node)?;
        let [declarator] = parts.as_slice() else {
            return Err(UNSUPPORTED);
        };
        if declarator.kind() != "variable_declarator" {
            return Err(UNSUPPORTED);
        }
        self.punctuation(*declarator, &["="])?;
        let children = self.parts(*declarator)?;
        let [name, value] = children.as_slice() else {
            return Err(UNSUPPORTED);
        };
        if declarator.child_by_field_name("name") != Some(*name)
            || declarator.child_by_field_name("value") != Some(*value)
        {
            return Err(UNSUPPORTED);
        }
        let name = self.identifier(*name)?;
        let mut cursor = node.walk();
        let keyword = node
            .children(&mut cursor)
            .find(|child| !child.is_named() && matches!(child.kind(), "const" | "let"))
            .ok_or(UNSUPPORTED)?;
        Ok((name, *value, keyword.kind() == "const"))
    }

    pub(super) fn statement_expression<'tree>(&self, node: Node<'tree>) -> Result<Node<'tree>> {
        self.punctuation(node, &[";"])?;
        let parts = self.parts(node)?;
        let [value] = parts.as_slice() else {
            return Err(UNSUPPORTED);
        };
        Ok(*value)
    }
}
