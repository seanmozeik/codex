//! Structural expressions only; origin tracking belongs to the shared extractor.

use super::Lowerer;
use super::field;
use super::only;
use super::parts;
use super::span;
use crate::source_description::python::origin::literal;
use crate::source_description::python::origin::shared::BinaryOp;
use crate::source_description::python::origin::shared::Expr;
use crate::source_description::python::origin::shared::ExprKind;
use crate::source_description::python::origin::shared::Fallback;
use crate::source_description::python::origin::shared::Index;
use crate::source_description::python::origin::shared::UnaryOp;
use tree_sitter::Node;

impl Lowerer<'_> {
    pub(super) fn expression(
        &mut self,
        mut node: Node<'_>,
        depth: usize,
    ) -> Result<Expr, Fallback> {
        // Parentheses are absent from Ruff's AST. Erase without recursive stack
        // growth or spending IR budget on parser-specific wrapper nodes.
        while node.kind() == "parenthesized_expression" {
            self.limits.check()?;
            node = only(node)?;
        }
        self.budget.node(depth, 128)?;
        let kind = match node.kind() {
            "identifier" => {
                let name = self.name(node)?;
                self.budget.node(depth, name.len())?;
                ExprKind::Name(name.to_owned())
            }
            "string" => {
                let raw = self.text(node)?;
                self.budget
                    .node(depth, raw.len().checked_mul(2).ok_or(Fallback::Limit)?)?;
                ExprKind::Text(literal::decode(raw)?)
            }
            "integer" => ExprKind::Integer(literal::integer(self.text(node)?)?),
            "list" => {
                let mut values = Vec::new();
                for child in parts(node) {
                    values.push(self.expression(child, depth + 1)?);
                }
                ExprKind::List(values)
            }
            "attribute" => {
                let value = self.expression(field(node, "object")?, depth + 1)?;
                let name = self.name(field(node, "attribute")?)?;
                self.budget.node(depth, name.len())?;
                ExprKind::Attribute {
                    value: Box::new(value),
                    name: name.to_owned(),
                }
            }
            "call" => self.call(node, depth)?,
            "binary_operator" => {
                let op = match self.text(field(node, "operator")?)? {
                    "+" => BinaryOp::Add,
                    "-" => BinaryOp::Subtract,
                    _ => return Err(Fallback::Unsupported),
                };
                let left = self.expression(field(node, "left")?, depth + 1)?;
                let right = self.expression(field(node, "right")?, depth + 1)?;
                ExprKind::Binary {
                    left: Box::new(left),
                    op,
                    right: Box::new(right),
                }
            }
            "unary_operator" => {
                let op = match self.text(field(node, "operator")?)? {
                    "+" => UnaryOp::Plus,
                    "-" => UnaryOp::Minus,
                    _ => return Err(Fallback::Unsupported),
                };
                let value = self.expression(field(node, "argument")?, depth + 1)?;
                ExprKind::Unary {
                    op,
                    value: Box::new(value),
                }
            }
            "subscript" => self.subscript(node, depth)?,
            _ => return Err(Fallback::Unsupported),
        };
        Ok(Expr {
            span: span(node),
            kind,
        })
    }

    fn call(&mut self, node: Node<'_>, depth: usize) -> Result<ExprKind, Fallback> {
        let function = self.expression(field(node, "function")?, depth + 1)?;
        let arguments = field(node, "arguments")?;
        if arguments.kind() != "argument_list" {
            return Err(Fallback::Unsupported);
        }
        let mut args = Vec::new();
        let mut keywords: Vec<(String, Expr)> = Vec::new();
        for argument in parts(arguments) {
            if argument.kind() == "keyword_argument" {
                let name = self.name(field(argument, "name")?)?;
                if keywords.iter().any(|(existing, _)| existing == name) {
                    return Err(Fallback::Syntax);
                }
                self.budget.node(depth, name.len())?;
                let value = self.expression(field(argument, "value")?, depth + 1)?;
                keywords.push((name.to_owned(), value));
            } else {
                if !keywords.is_empty() {
                    return Err(Fallback::Syntax);
                }
                args.push(self.expression(argument, depth + 1)?);
            }
        }
        Ok(ExprKind::Call {
            function: Box::new(function),
            args,
            keywords,
        })
    }

    fn subscript(&mut self, node: Node<'_>, depth: usize) -> Result<ExprKind, Fallback> {
        let mut cursor = node.walk();
        // A trailing comma denotes a one-element tuple, not an item index.
        if node.children(&mut cursor).any(|child| child.kind() == ",") {
            return Err(Fallback::Unsupported);
        }
        if parts(node).count() != 2 {
            return Err(Fallback::Unsupported);
        }
        let value = self.expression(field(node, "value")?, depth + 1)?;
        let index = field(node, "subscript")?;
        self.budget.node(depth + 1, 128)?;
        let index = if index.kind() == "slice" {
            self.slice(index, depth + 2)?
        } else {
            Index::Item(Box::new(self.expression(index, depth + 2)?))
        };
        Ok(ExprKind::Subscript {
            value: Box::new(value),
            index,
        })
    }

    fn slice(&mut self, node: Node<'_>, depth: usize) -> Result<Index, Fallback> {
        let mut slots: [Option<Box<Expr>>; 3] = [None, None, None];
        let mut slot = 0;
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "comment" | "line_continuation" => {}
                ":" if slot < 2 => slot += 1,
                _ if child.is_named() && slots[slot].is_none() => {
                    slots[slot] = Some(Box::new(self.expression(child, depth)?));
                }
                _ => return Err(Fallback::Unsupported),
            }
        }
        if slot == 0 {
            return Err(Fallback::Unsupported);
        }
        let [lower, upper, step] = slots;
        Ok(Index::Slice { lower, upper, step })
    }
}
