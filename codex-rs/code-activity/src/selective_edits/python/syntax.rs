//! Whole-source, finite compilation-shape checks before retaining any prefix.

pub(super) mod contexts;
mod expressions;
mod lexical;
pub(super) mod predicates;

use super::literal;
use crate::SourceDescriptionFallback as Fallback;
use crate::source_description::DescriptionBudget;
use tree_sitter::Node;

pub(super) fn validate(
    root: Node<'_>,
    source: &str,
    budget: DescriptionBudget,
) -> Result<(), Fallback> {
    if root.kind() != "module" || source.contains(['\0', '\r', '\x0c']) {
        return Err(Fallback::UnsupportedSource);
    }
    lexical::lexical_gaps(root, source, budget)?;
    let checker = Checker { source, budget };
    for statement in parts(root) {
        checker.indentation(statement, 0)?;
        checker.statement(statement, false)?;
    }
    budget.check()
}

struct Checker<'s> {
    source: &'s str,
    budget: DescriptionBudget,
}

impl Checker<'_> {
    fn indentation(&self, node: Node<'_>, expected: usize) -> Result<(), Fallback> {
        let before = &self.source[..node.start_byte()];
        let line_start = before.rfind('\n').map_or(0, |index| index + 1);
        let prefix = &self.source[line_start..node.start_byte()];
        // Only a first statement on its physical line establishes indentation.
        // Later semicolon-separated statements have code in this prefix.
        if prefix.bytes().all(|byte| matches!(byte, b' ' | b'\t'))
            && (prefix.contains('\t') || prefix.len() != expected)
        {
            return Err(Fallback::UnsupportedSource);
        }
        Ok(())
    }

    fn statement(&self, node: Node<'_>, in_function: bool) -> Result<(), Fallback> {
        self.budget.check()?;
        match node.kind() {
            "pass_statement" if parts(node).next().is_none() => Ok(()),
            "expression_statement" => {
                let value = only(node)?;
                if value.kind() == "assignment" {
                    if value.child_by_field_name("type").is_some() || parts(value).count() != 2 {
                        return Err(Fallback::UnsupportedSource);
                    }
                    self.assignment_target(field(value, "left")?)?;
                    self.expression(field(value, "right")?)
                } else if value.kind() == "augmented_assignment" {
                    // Only a plain local name and immutable-text += are
                    // modeled. Reject other targets/operators for the whole
                    // module before evaluating or retaining any prefix.
                    if parts(value).count() != 2
                        || crate::text(field(value, "operator")?, self.source) != "+="
                    {
                        return Err(Fallback::UnsupportedSource);
                    }
                    self.name(field(value, "left")?)?;
                    self.expression(field(value, "right")?)
                } else {
                    self.expression(value)
                }
            }
            "return_statement" if in_function => {
                let mut values = parts(node);
                if let Some(value) = values.next() {
                    if values.next().is_some() {
                        return Err(Fallback::UnsupportedSource);
                    }
                    self.expression(value)?;
                }
                Ok(())
            }
            "for_statement" if !in_function => self.literal_loop(node, 0),
            "with_statement" if !in_function => self.context(node),
            "function_definition" if !in_function => self.function(node),
            "import_statement" | "import_from_statement" if !in_function => self.import(node),
            _ => Err(Fallback::UnsupportedSource),
        }
    }

    fn literal_loop(&self, node: Node<'_>, depth: usize) -> Result<(), Fallback> {
        self.budget.check()?;
        if depth >= super::eval::loops::MAX_LOOP_DEPTH {
            return Err(Fallback::SourceLimit);
        }
        let shape = super::eval::loops::shape(node, self.source)?;
        for name in shape.names {
            self.name(name)?;
        }
        for row in shape.rows {
            for literal in row {
                self.expression(literal)?;
            }
        }
        // Validate every body even when the literal is empty. Imports,
        // definitions, return, break and continue are outside this loop subset.
        let first = parts(shape.body)
            .next()
            .ok_or(Fallback::UnsupportedSource)?;
        let inline = first.start_position().row == node.start_position().row;
        let indentation = first.start_position().column;
        if !inline && indentation <= node.start_position().column {
            return Err(Fallback::UnsupportedSource);
        }
        for statement in parts(shape.body) {
            if inline && statement.start_position().row != first.start_position().row {
                return Err(Fallback::UnsupportedSource);
            }
            self.indentation(statement, indentation)?;
            match statement.kind() {
                "for_statement" => self.literal_loop(statement, depth + 1)?,
                "pass_statement" | "expression_statement" => self.statement(statement, false)?,
                _ => return Err(Fallback::UnsupportedSource),
            }
        }
        Ok(())
    }

    fn assignment_target(&self, node: Node<'_>) -> Result<(), Fallback> {
        match node.kind() {
            "identifier" => self.name(node),
            "attribute" | "subscript" => self.expression(node),
            _ => Err(Fallback::UnsupportedSource),
        }
    }

    fn function(&self, node: Node<'_>) -> Result<(), Fallback> {
        if parts(node).count() != 3
            || node.child_by_field_name("return_type").is_some()
            || node.child_by_field_name("type_parameters").is_some()
        {
            return Err(Fallback::UnsupportedSource);
        }
        let mut cursor = node.walk();
        if node
            .children(&mut cursor)
            .any(|child| child.kind() == "async")
        {
            return Err(Fallback::UnsupportedSource);
        }
        self.name(field(node, "name")?)?;
        let parameters = field(node, "parameters")?;
        if parameters.kind() != "parameters" {
            return Err(Fallback::UnsupportedSource);
        }
        let mut cursor = parameters.walk();
        if parameters
            .children(&mut cursor)
            .any(|child| !child.is_named() && !matches!(child.kind(), "(" | ")" | ","))
        {
            return Err(Fallback::UnsupportedSource);
        }
        for (index, parameter) in parts(parameters).enumerate() {
            self.name(parameter)?;
            let name = crate::text(parameter, self.source);
            if parts(parameters)
                .take(index)
                .any(|earlier| crate::text(earlier, self.source) == name)
            {
                return Err(Fallback::UnsupportedSource);
            }
        }
        let body = field(node, "body")?;
        if body.kind() != "block" || parts(body).next().is_none() {
            return Err(Fallback::UnsupportedSource);
        }
        let first = parts(body).next().ok_or(Fallback::UnsupportedSource)?;
        let inline = first.start_position().row == node.start_position().row;
        let indentation = first.start_position().column;
        if !inline && indentation <= node.start_position().column {
            return Err(Fallback::UnsupportedSource);
        }
        for statement in parts(body) {
            if inline && statement.start_position().row != first.start_position().row {
                return Err(Fallback::UnsupportedSource);
            }
            self.indentation(statement, indentation)?;
            self.statement(statement, true)?;
        }
        Ok(())
    }

    fn import(&self, node: Node<'_>) -> Result<(), Fallback> {
        let mut cursor = node.walk();
        let last = node
            .children(&mut cursor)
            .filter(|child| !matches!(child.kind(), "comment" | "line_continuation"))
            .last();
        // The pinned grammar admits _import_list's trailing comma without the
        // parentheses required by Python. Inspect the anonymous punctuation too.
        if last.is_some_and(|child| child.kind() == ",") {
            return Err(Fallback::UnsupportedSource);
        }
        let mut items = parts(node);
        let from = node.kind() == "import_from_statement";
        if from {
            let module = items.next().ok_or(Fallback::UnsupportedSource)?;
            self.dotted_name(module, false)?;
            if crate::text(module, self.source) == "__future__" {
                return Err(Fallback::UnsupportedSource);
            }
        }
        let mut count = 0;
        for item in items {
            count += 1;
            match item.kind() {
                "dotted_name" => self.dotted_name(item, from)?,
                "aliased_import" if parts(item).count() == 2 => {
                    self.dotted_name(field(item, "name")?, from)?;
                    self.name(field(item, "alias")?)?;
                }
                "wildcard_import" if from && parts(item).next().is_none() => {}
                _ => return Err(Fallback::UnsupportedSource),
            }
        }
        if count == 0 {
            return Err(Fallback::UnsupportedSource);
        }
        Ok(())
    }

    fn dotted_name(&self, node: Node<'_>, single: bool) -> Result<(), Fallback> {
        if node.kind() != "dotted_name"
            || parts(node).next().is_none()
            || (single && parts(node).count() != 1)
        {
            return Err(Fallback::UnsupportedSource);
        }
        for item in parts(node) {
            self.name(item)?;
        }
        Ok(())
    }

    fn name(&self, node: Node<'_>) -> Result<(), Fallback> {
        if node.kind() == "identifier" && literal::identifier(crate::text(node, self.source)) {
            Ok(())
        } else {
            Err(Fallback::UnsupportedSource)
        }
    }
}

pub(super) fn parts(node: Node<'_>) -> impl Iterator<Item = Node<'_>> {
    (0..node.named_child_count()).filter_map(move |index| {
        node.named_child(index)
            .filter(|child| !matches!(child.kind(), "comment" | "line_continuation"))
    })
}

pub(super) fn conditional_parts(
    node: Node<'_>,
) -> Result<(Node<'_>, Node<'_>, Node<'_>), Fallback> {
    // The pinned grammar has no field names for these three named children.
    let mut children = parts(node);
    let consequence = children.next().ok_or(Fallback::UnsupportedSource)?;
    let predicate = children.next().ok_or(Fallback::UnsupportedSource)?;
    let alternative = children.next().ok_or(Fallback::UnsupportedSource)?;
    if children.next().is_some() {
        return Err(Fallback::UnsupportedSource);
    }
    Ok((consequence, predicate, alternative))
}

pub(super) fn field<'a>(node: Node<'a>, name: &str) -> Result<Node<'a>, Fallback> {
    node.child_by_field_name(name)
        .ok_or(Fallback::UnsupportedSource)
}

pub(super) fn only(node: Node<'_>) -> Result<Node<'_>, Fallback> {
    let mut children = parts(node);
    let child = children.next().ok_or(Fallback::UnsupportedSource)?;
    if children.next().is_some() {
        return Err(Fallback::UnsupportedSource);
    }
    Ok(child)
}

pub(super) fn unparenthesized(mut node: Node<'_>) -> Result<Node<'_>, Fallback> {
    while node.kind() == "parenthesized_expression" {
        node = only(node)?;
    }
    Ok(node)
}
