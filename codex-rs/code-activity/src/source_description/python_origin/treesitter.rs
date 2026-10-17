//! Lower the pinned Python CST to the shared, source-only owned syntax IR.
//! Native parsing and full-CST limits remain in the enclosing production adapter.

#[path = "treesitter_expr.rs"]
mod expressions;

use crate::source_description::python::origin::literal;
use crate::source_description::python::origin::shared::Budget;
use crate::source_description::python::origin::shared::Fallback;
use crate::source_description::python::origin::shared::Limits;
use crate::source_description::python::origin::shared::Module;
use crate::source_description::python::origin::shared::Span;
use crate::source_description::python::origin::shared::Statement;
use crate::source_description::python::origin::shared::StatementKind;
use tree_sitter::Node;
use tree_sitter::Tree;

/// Lower a native tree for the same exact source. The caller owns that pairing.
pub fn lower(source: &str, tree: &Tree, limits: Limits) -> Result<Module, Fallback> {
    let budget = Budget::new(source, limits)?;
    let root = tree.root_node();
    if source.contains('\0') || root.has_error() || root.kind() != "module" {
        return Err(Fallback::Syntax);
    }
    let mut lowerer = Lowerer {
        source,
        budget,
        limits,
    };
    lowerer.budget.node(0, 64)?;
    let mut statements = Vec::new();
    for node in parts(root) {
        statements.push(lowerer.statement(node, 1, true)?);
    }
    limits.check()?;
    Ok(Module { statements })
}

struct Lowerer<'a> {
    source: &'a str,
    budget: Budget,
    limits: Limits,
}

impl<'s> Lowerer<'s> {
    fn statement(
        &mut self,
        node: Node<'_>,
        depth: usize,
        allow_with: bool,
    ) -> Result<Statement, Fallback> {
        self.budget.node(depth, 128)?;
        let mut evidence = span(node);
        let kind = match node.kind() {
            "import_from_statement" => {
                let alias = self.import_alias(node)?;
                self.budget.node(depth, alias.len())?;
                StatementKind::ImportPath {
                    alias: alias.to_owned(),
                }
            }
            "expression_statement" => {
                let expression = only(node)?;
                if expression.kind() == "assignment" {
                    if expression.child_by_field_name("type").is_some() {
                        return Err(Fallback::Unsupported);
                    }
                    if parts(expression).count() != 2 {
                        return Err(Fallback::Unsupported);
                    }
                    let name = self.target_name(field(expression, "left")?)?;
                    self.budget.node(depth, name.len())?;
                    let value = self.expression(field(expression, "right")?, depth + 1)?;
                    StatementKind::Assign {
                        name: name.to_owned(),
                        value,
                    }
                } else {
                    StatementKind::Expression(self.expression(expression, depth + 1)?)
                }
            }
            "with_statement" if allow_with => {
                let (name, context, block) = self.with_parts(node)?;
                self.budget.node(depth, name.len())?;
                let context = self.expression(context, depth + 1)?;
                let mut body = Vec::new();
                for statement in parts(block) {
                    body.push(self.statement(statement, depth + 1, false)?);
                }
                // A CST block can include trailing comments; Ruff's statement
                // range ends with the last actual statement.
                evidence.end = body.last().ok_or(Fallback::Unsupported)?.span.end;
                StatementKind::With {
                    name: name.to_owned(),
                    context,
                    body,
                }
            }
            _ => return Err(Fallback::Unsupported),
        };
        Ok(Statement {
            span: evidence,
            kind,
        })
    }

    fn import_alias(&self, node: Node<'_>) -> Result<&'s str, Fallback> {
        let [module, item] = pair(node)?;
        if module.kind() != "dotted_name" || self.text(module)? != "pathlib" {
            return Err(Fallback::Unsupported);
        }
        match item.kind() {
            "dotted_name" if self.text(item)? == "Path" => Ok("Path"),
            "aliased_import" => {
                let original = field(item, "name")?;
                if self.text(original)? != "Path" || parts(item).count() != 2 {
                    return Err(Fallback::Unsupported);
                }
                self.name(field(item, "alias")?)
            }
            _ => Err(Fallback::Unsupported),
        }
    }

    fn with_parts<'a>(&self, node: Node<'a>) -> Result<(&'s str, Node<'a>, Node<'a>), Fallback> {
        let mut cursor = node.walk();
        if node
            .children(&mut cursor)
            .any(|child| child.kind() == "async")
        {
            return Err(Fallback::Unsupported);
        }
        let [clause, block] = pair(node)?;
        if clause.kind() != "with_clause" || block.kind() != "block" {
            return Err(Fallback::Unsupported);
        }
        let item = only(clause)?;
        if item.kind() != "with_item" {
            return Err(Fallback::Unsupported);
        }
        let mut pattern = field(item, "value")?;
        // The pinned grammar can represent the context-list parentheses as an
        // expression wrapper around the one `context as target` item.
        while pattern.kind() == "parenthesized_expression" {
            self.limits.check()?;
            pattern = only(pattern)?;
        }
        if pattern.kind() != "as_pattern" {
            return Err(Fallback::Unsupported);
        }
        let [context, target] = pair(pattern)?;
        if target.kind() != "as_pattern_target" {
            return Err(Fallback::Unsupported);
        }
        Ok((self.target_name(only(target)?)?, context, block))
    }

    fn target_name(&self, mut node: Node<'_>) -> Result<&'s str, Fallback> {
        while matches!(node.kind(), "tuple_pattern" | "parenthesized_expression") {
            self.limits.check()?;
            let mut cursor = node.walk();
            if node.children(&mut cursor).any(|child| child.kind() == ",") {
                return Err(Fallback::Unsupported);
            }
            node = only(node)?;
        }
        self.name(node)
    }

    fn name(&self, node: Node<'_>) -> Result<&'s str, Fallback> {
        let name = self.text(node)?;
        if node.kind() == "identifier" && literal::identifier(name) {
            Ok(name)
        } else {
            Err(Fallback::Unsupported)
        }
    }

    fn text(&self, node: Node<'_>) -> Result<&'s str, Fallback> {
        self.source.get(node.byte_range()).ok_or(Fallback::Syntax)
    }
}

fn span(node: Node<'_>) -> Span {
    Span {
        start: node.start_byte(),
        end: node.end_byte(),
    }
}

fn field<'a>(node: Node<'a>, name: &str) -> Result<Node<'a>, Fallback> {
    node.child_by_field_name(name).ok_or(Fallback::Unsupported)
}

fn parts(node: Node<'_>) -> impl Iterator<Item = Node<'_>> {
    // No collection is allocated before the shared node budget is charged.
    (0..node.named_child_count()).filter_map(move |index| {
        node.named_child(index)
            .filter(|child| !matches!(child.kind(), "comment" | "line_continuation"))
    })
}

fn only(node: Node<'_>) -> Result<Node<'_>, Fallback> {
    let mut children = parts(node);
    let child = children.next().ok_or(Fallback::Unsupported)?;
    if children.next().is_some() {
        return Err(Fallback::Unsupported);
    }
    Ok(child)
}

fn pair(node: Node<'_>) -> Result<[Node<'_>; 2], Fallback> {
    let mut children = parts(node);
    let first = children.next().ok_or(Fallback::Unsupported)?;
    let second = children.next().ok_or(Fallback::Unsupported)?;
    if children.next().is_some() {
        return Err(Fallback::Unsupported);
    }
    Ok([first, second])
}
