//! Full-source expression shape checks, independent of runtime reachability.

use super::Checker;
use super::conditional_parts;
use super::field;
use super::only;
use super::parts;
use super::predicates::boolean_parts;
use super::predicates::comparison_parts;
use super::unparenthesized;
use crate::SourceDescriptionFallback as Fallback;
use crate::selective_edits::python::literal;
use tree_sitter::Node;

impl Checker<'_> {
    pub(super) fn expression(&self, node: Node<'_>) -> Result<(), Fallback> {
        self.budget.check()?;
        match node.kind() {
            "identifier" => self.name(node),
            "string" => literal::decode(crate::text(node, self.source)).map(|_| ()),
            "integer" => literal::integer(crate::text(node, self.source)).map(|_| ()),
            "true" | "false" | "none" => Ok(()),
            "parenthesized_expression" => self.expression(only(node)?),
            "list" | "tuple" => {
                for item in parts(node) {
                    self.expression(item)?;
                }
                Ok(())
            }
            "dictionary" => {
                for item in parts(node) {
                    if item.kind() != "pair" || parts(item).count() != 2 {
                        return Err(Fallback::UnsupportedSource);
                    }
                    self.expression(field(item, "key")?)?;
                    self.expression(field(item, "value")?)?;
                }
                Ok(())
            }
            "attribute" if parts(node).count() == 2 => {
                self.expression(field(node, "object")?)?;
                self.name(field(node, "attribute")?)
            }
            "subscript" if parts(node).count() == 2 => {
                self.expression(field(node, "value")?)?;
                self.expression(field(node, "subscript")?)
            }
            "slice" => {
                let mut cursor = node.walk();
                if node
                    .children(&mut cursor)
                    .filter(|child| child.kind() == ":")
                    .count()
                    != 1
                    || parts(node).count() > 2
                {
                    return Err(Fallback::UnsupportedSource);
                }
                for bound in parts(node) {
                    self.expression(bound)?;
                }
                Ok(())
            }
            "call" if parts(node).count() == 2 => self.call(node),
            "conditional_expression" => {
                let (consequence, predicate, alternative) = conditional_parts(node)?;
                // Both arms must compile even though only one executes.
                self.expression(consequence)?;
                self.expression(predicate)?;
                self.expression(alternative)
            }
            "comparison_operator" => {
                let (_, left, right) = comparison_parts(node, self.source)?;
                self.expression(left)?;
                self.expression(right)
            }
            "boolean_operator" => {
                let (left, right) = boolean_parts(node, self.source)?;
                // Validate both operands before any prefix can be retained.
                self.expression(left)?;
                self.expression(right)
            }
            "binary_operator" if parts(node).count() == 2 => {
                if crate::text(field(node, "operator")?, self.source) != "+" {
                    return Err(Fallback::UnsupportedSource);
                }
                self.expression(field(node, "left")?)?;
                self.expression(field(node, "right")?)
            }
            "unary_operator" if parts(node).count() == 1 => {
                if !matches!(
                    crate::text(field(node, "operator")?, self.source),
                    "+" | "-"
                ) {
                    return Err(Fallback::UnsupportedSource);
                }
                let argument = unparenthesized(field(node, "argument")?)?;
                if argument.kind() != "integer" {
                    return Err(Fallback::UnsupportedSource);
                }
                self.expression(argument)
            }
            _ => Err(Fallback::UnsupportedSource),
        }
    }

    fn call(&self, node: Node<'_>) -> Result<(), Fallback> {
        self.expression(field(node, "function")?)?;
        let arguments = field(node, "arguments")?;
        if arguments.kind() != "argument_list" {
            return Err(Fallback::UnsupportedSource);
        }
        let mut has_keyword = false;
        for (index, argument) in parts(arguments).enumerate() {
            if argument.kind() == "keyword_argument" && parts(argument).count() == 2 {
                has_keyword = true;
                let name = field(argument, "name")?;
                self.name(name)?;
                if parts(arguments).take(index).any(|earlier| {
                    earlier.kind() == "keyword_argument"
                        && earlier
                            .child_by_field_name("name")
                            .is_some_and(|earlier_name| {
                                crate::text(earlier_name, self.source)
                                    == crate::text(name, self.source)
                            })
                }) {
                    return Err(Fallback::UnsupportedSource);
                }
                self.expression(field(argument, "value")?)?;
            } else {
                if has_keyword {
                    return Err(Fallback::UnsupportedSource);
                }
                self.expression(argument)?;
            }
        }
        Ok(())
    }
}
