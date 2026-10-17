//! Exact display syntax; callback bodies cannot contain even nested awaits.

use super::MAX_ITEMS;
use super::Reader;
use super::Result;
use super::SourceDescriptionFallback;
use super::SourceReferenceKind;
use super::UNSUPPORTED;
use super::syntax;
use super::values::DisplayScope;
use super::values::LocalBindings;
use tree_sitter::Node;

impl Reader<'_> {
    pub(super) fn display(&mut self, node: Node<'_>) -> Result<()> {
        let (function, arguments) = syntax::call(node, self.budget)?;
        if function.kind() == "identifier" {
            self.text_call(node, DisplayScope::Outer)?;
        } else {
            self.for_each(function, arguments)?;
        }
        self.reference(node, SourceReferenceKind::OutputConsumer, None)
    }

    fn for_each(&mut self, function: Node<'_>, arguments: Node<'_>) -> Result<()> {
        let (_, method) = syntax::member(function, self.source, self.budget)?;
        if method != "forEach" {
            return Err(UNSUPPORTED);
        }
        let joined = self
            .binding_origin(syntax::field(function, "object")?)?
            .joined()?;
        let arguments = syntax::sequence(arguments, "(", ")", self.budget)?;
        let [callback] = arguments.as_slice() else {
            return Err(UNSUPPORTED);
        };
        if callback.kind() == "identifier" {
            return if syntax::identifier(*callback, self.source)? == "text" {
                Ok(())
            } else {
                Err(UNSUPPORTED)
            };
        }
        if callback.kind() != "arrow_function" || syntax::parts(*callback, self.budget)?.len() != 2
        {
            return Err(UNSUPPORTED);
        }
        syntax::punctuation(*callback, &["=>"], self.budget)?;
        let parameters = self.arrow_parameters(*callback)?;
        let (element, index) = match parameters.as_slice() {
            [element] => (*element, None),
            [element, index] => (*element, Some(*index)),
            _ => return Err(UNSUPPORTED),
        };
        self.fresh_name(element)?;
        let source = self.source;
        let element_name = syntax::identifier(element, source)?;
        let index_name = if let Some(index) = index {
            self.fresh_name(index)?;
            let name = syntax::identifier(index, source)?;
            if name == element_name {
                return Err(UNSUPPORTED);
            }
            Some(name)
        } else {
            None
        };
        let locals = LocalBindings {
            element_name,
            element_origin: joined.element(),
            index_name,
        };
        let body = syntax::field(*callback, "body")?;
        let legacy = index_name.is_none() && self.legacy_text(body, element_name)?;
        if !legacy {
            let name_bytes = element_name
                .len()
                .checked_add(index_name.map_or(0, str::len))
                .ok_or(SourceDescriptionFallback::SourceLimit)?;
            self.extension(1, name_bytes)?;
        }
        self.display_body(body, locals, legacy, BodyPosition::Arrow)
    }

    fn arrow_parameters<'tree>(&self, callback: Node<'tree>) -> Result<Vec<Node<'tree>>> {
        if let Some(parameter) = callback.child_by_field_name("parameter") {
            syntax::identifier(parameter, self.source)?;
            return Ok(vec![parameter]);
        }
        let parameters = syntax::field(callback, "parameters")?;
        if parameters.kind() != "formal_parameters" {
            return Err(UNSUPPORTED);
        }
        let mut identifiers = Vec::new();
        for parameter in syntax::sequence(parameters, "(", ")", self.budget)? {
            if parameter.kind() != "required_parameter" {
                return Err(UNSUPPORTED);
            }
            syntax::punctuation(parameter, &[], self.budget)?;
            let parts = syntax::parts(parameter, self.budget)?;
            let [identifier] = parts.as_slice() else {
                return Err(UNSUPPORTED);
            };
            syntax::identifier(*identifier, self.source)?;
            identifiers.push(*identifier);
        }
        Ok(identifiers)
    }

    fn text_call(&mut self, node: Node<'_>, scope: DisplayScope<'_>) -> Result<()> {
        let argument = self.text_argument(node)?;
        self.display_value(argument, scope)?;
        Ok(())
    }

    fn text_argument<'tree>(&self, node: Node<'tree>) -> Result<Node<'tree>> {
        let (function, arguments) = syntax::call(node, self.budget)?;
        if syntax::identifier(function, self.source)? != "text" {
            return Err(UNSUPPORTED);
        }
        let arguments = syntax::sequence(arguments, "(", ")", self.budget)?;
        let [argument] = arguments.as_slice() else {
            return Err(UNSUPPORTED);
        };
        Ok(*argument)
    }

    fn legacy_text(&self, node: Node<'_>, name: &str) -> Result<bool> {
        match self.text_argument(node) {
            Ok(argument) => Ok(argument.kind() == "identifier"
                && syntax::identifier(argument, self.source)? == name),
            Err(SourceDescriptionFallback::UnsupportedSource) => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn expression<'tree>(&self, node: Node<'tree>) -> Result<Node<'tree>> {
        if node.kind() != "expression_statement" {
            return Err(UNSUPPORTED);
        }
        syntax::punctuation(node, &[";"], self.budget)?;
        let parts = syntax::parts(node, self.budget)?;
        let [expression] = parts.as_slice() else {
            return Err(UNSUPPORTED);
        };
        Ok(*expression)
    }

    fn display_body(
        &mut self,
        body: Node<'_>,
        locals: LocalBindings<'_>,
        legacy: bool,
        position: BodyPosition,
    ) -> Result<()> {
        let scope = DisplayScope::Body(locals);
        if body.kind() == "statement_block" {
            syntax::punctuation(body, &["{", "}"], self.budget)?;
            let statements = syntax::parts(body, self.budget)?;
            if statements.len() > MAX_ITEMS {
                return Err(SourceDescriptionFallback::SourceLimit);
            }
            if !legacy {
                self.extension(1, 0)?;
            }
            for statement in statements {
                if !legacy {
                    self.extension(1, 0)?;
                }
                if statement.kind() == "empty_statement" {
                    if !syntax::parts(statement, self.budget)?.is_empty() {
                        return Err(UNSUPPORTED);
                    }
                    syntax::punctuation(statement, &[";"], self.budget)?;
                } else {
                    self.text_call(self.expression(statement)?, scope)?;
                }
            }
            Ok(())
        } else {
            if !legacy {
                self.extension(1, 0)?;
            }
            let expression = match position {
                BodyPosition::Arrow => body,
                BodyPosition::Loop => self.expression(body)?,
            };
            self.text_call(expression, scope)
        }
    }

    pub(super) fn display_loop(&mut self, node: Node<'_>) -> Result<()> {
        if syntax::field(node, "kind")?.kind() != "const"
            || syntax::field(node, "operator")?.kind() != "of"
            || syntax::parts(node, self.budget)?.len() != 3
        {
            return Err(UNSUPPORTED);
        }
        syntax::punctuation(node, &["for", "(", "const", "of", ")"], self.budget)?;
        let left = syntax::field(node, "left")?;
        self.fresh_name(left)?;
        let source = self.source;
        let name = syntax::identifier(left, source)?;
        let joined = self
            .binding_origin(syntax::field(node, "right")?)?
            .joined()?;
        let body = syntax::field(node, "body")?;
        let legacy_statement = if body.kind() == "statement_block" {
            syntax::punctuation(body, &["{", "}"], self.budget)?;
            let parts = syntax::parts(body, self.budget)?;
            if let [statement] = parts.as_slice() {
                Some(*statement)
            } else {
                None
            }
        } else {
            Some(body)
        };
        let legacy = if let Some(statement) = legacy_statement {
            if statement.kind() == "expression_statement" {
                self.legacy_text(self.expression(statement)?, name)?
            } else {
                false
            }
        } else {
            false
        };
        if !legacy {
            self.extension(1, name.len())?;
        }
        self.display_body(
            body,
            LocalBindings {
                element_name: name,
                element_origin: joined.element(),
                index_name: None,
            },
            legacy,
            BodyPosition::Loop,
        )?;
        self.reference(node, SourceReferenceKind::OutputConsumer, None)
    }
}

#[derive(Clone, Copy)]
enum BodyPosition {
    Arrow,
    Loop,
}
