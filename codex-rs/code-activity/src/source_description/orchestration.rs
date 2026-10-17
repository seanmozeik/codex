//! Closed outer-source recognition; the parent must validate every shell child.

#[path = "orchestration_calls.rs"]
mod calls;
#[path = "orchestration_display.rs"]
mod display;
#[path = "orchestration_strings.rs"]
mod strings;
#[path = "orchestration_syntax.rs"]
mod syntax;
#[cfg(test)]
#[path = "orchestration_tests.rs"]
mod tests;
#[path = "orchestration_values.rs"]
mod values;

use super::DescriptionBudget;
use super::MAX_REFERENCES;
use super::MAX_RETAINED_BYTES;
use super::SourceDescriptionFallback;
use super::SourceReference;
use super::SourceReferenceBasis;
use super::SourceReferenceKind;
use super::patch::LiteralPatchInvocation;
use crate::SourceId;
use crate::Span;
use std::collections::BTreeMap;
use tree_sitter::Node;
use values::ValueOrigin;

const MAX_ITEMS: usize = 8;
const MAX_TEXT: usize = 16_320;
const MAX_EXTENSION_EVENTS: usize = 4096;
const UNSUPPORTED: SourceDescriptionFallback = SourceDescriptionFallback::UnsupportedSource;
type Result<T> = std::result::Result<T, SourceDescriptionFallback>;

pub(super) enum Invocation {
    Command { command: String, evidence: Span },
    Patch(LiteralPatchInvocation),
    Poll(ValidatedPoll),
}

/// Literal empty polling has no owned code source or inferred session behavior.
pub(super) struct ValidatedPoll;

pub(super) struct Plan {
    pub invocations: Vec<Invocation>,
    pub references: Vec<SourceReference>,
    pub extension_events: usize,
    pub extension_name_bytes: usize,
}

enum Binding {
    Text(String),
    Display(ValueOrigin),
}

struct Reader<'a> {
    source: &'a str,
    budget: DescriptionBudget,
    bindings: BTreeMap<String, Binding>,
    plan: Plan,
    groups: usize,
    retained_bytes: usize,
    json_shadowed: bool,
}

pub(super) fn extract(root: Node<'_>, source: &str, budget: DescriptionBudget) -> Result<Plan> {
    budget.check()?;
    if root.kind() != "program" || root.has_error() {
        return Err(UNSUPPORTED);
    }
    let statements = syntax::parts(root, budget)?;
    let json_shadowed = syntax::declares_json(&statements, source, budget)?;
    let mut reader = Reader {
        source,
        budget,
        bindings: BTreeMap::new(),
        plan: Plan {
            invocations: vec![],
            references: vec![],
            extension_events: 0,
            extension_name_bytes: 0,
        },
        groups: 0,
        retained_bytes: 0,
        json_shadowed,
    };
    for statement in statements {
        reader.statement(statement)?;
    }
    budget.check()?;
    Ok(reader.plan)
}

impl Reader<'_> {
    fn statement(&mut self, node: Node<'_>) -> Result<()> {
        self.budget.check()?;
        match node.kind() {
            "lexical_declaration" => self.declaration(node),
            "expression_statement" => {
                syntax::punctuation(node, &[";"], self.budget)?;
                let parts = syntax::parts(node, self.budget)?;
                let [expression] = parts.as_slice() else {
                    return Err(UNSUPPORTED);
                };
                if matches!(
                    expression.kind(),
                    "await_expression" | "parenthesized_expression"
                ) {
                    self.await_statement(*expression)?;
                    Ok(())
                } else {
                    self.display(*expression)
                }
            }
            "for_in_statement" => self.display_loop(node),
            "empty_statement" if syntax::parts(node, self.budget)?.is_empty() => {
                syntax::punctuation(node, &[";"], self.budget)
            }
            _ => Err(UNSUPPORTED),
        }
    }

    fn declaration(&mut self, node: Node<'_>) -> Result<()> {
        if self.bindings.len() >= MAX_ITEMS {
            return Err(SourceDescriptionFallback::SourceLimit);
        }
        if node
            .child_by_field_name("kind")
            .is_none_or(|kind| kind.kind() != "const")
        {
            return Err(UNSUPPORTED);
        }
        syntax::punctuation(node, &["const", ";"], self.budget)?;
        let parts = syntax::parts(node, self.budget)?;
        let [declaration] = parts.as_slice() else {
            return Err(UNSUPPORTED);
        };
        if declaration.kind() != "variable_declarator"
            || syntax::parts(*declaration, self.budget)?.len() != 2
        {
            return Err(UNSUPPORTED);
        }
        syntax::punctuation(*declaration, &["="], self.budget)?;
        let name = syntax::field(*declaration, "name")?;
        let value = syntax::field(*declaration, "value")?;
        let key = self.fresh_name(name)?.to_owned();
        let binding = if value.kind() == "await_expression" {
            Binding::Display(self.awaited(value)?)
        } else if self.is_text_initializer(value)? {
            let value = self.string(value)?;
            self.retain(value.len())?;
            Binding::Text(value)
        } else {
            self.extension(1, 0)?;
            Binding::Display(self.display_value(value, values::DisplayScope::Outer)?)
        };
        self.retain(key.len())?;
        self.bindings.insert(key, binding);
        Ok(())
    }

    fn fresh_name(&self, node: Node<'_>) -> Result<&str> {
        let name = syntax::identifier(node, self.source)?;
        if syntax::reserved(name) || self.bindings.contains_key(name) {
            Err(UNSUPPORTED)
        } else {
            Ok(name)
        }
    }

    fn string(&self, node: Node<'_>) -> Result<String> {
        self.budget.check()?;
        match node.kind() {
            "string" => syntax::decode(crate::text(node, self.source), self.budget),
            "template_string" => strings::template(node, self.source, self.budget),
            "identifier" => match self.bindings.get(syntax::identifier(node, self.source)?) {
                Some(Binding::Text(value)) => Ok(value.clone()),
                _ => Err(UNSUPPORTED),
            },
            _ => Err(UNSUPPORTED),
        }
    }

    fn is_text_initializer(&self, node: Node<'_>) -> Result<bool> {
        Ok(match node.kind() {
            "string" | "template_string" => true,
            "identifier" => matches!(
                self.bindings.get(syntax::identifier(node, self.source)?),
                Some(Binding::Text(_))
            ),
            _ => false,
        })
    }

    fn binding_origin(&self, node: Node<'_>) -> Result<ValueOrigin> {
        match self.bindings.get(syntax::identifier(node, self.source)?) {
            Some(Binding::Display(origin)) => Ok(*origin),
            Some(Binding::Text(_)) => Ok(ValueOrigin::Scalar),
            _ => Err(UNSUPPORTED),
        }
    }

    fn retain(&mut self, bytes: usize) -> Result<()> {
        self.retained_bytes = self
            .retained_bytes
            .checked_add(bytes)
            .ok_or(SourceDescriptionFallback::SourceLimit)?;
        self.check_retained()
    }

    fn extension(&mut self, events: usize, name_bytes: usize) -> Result<()> {
        self.budget.check()?;
        self.plan.extension_events = self
            .plan
            .extension_events
            .checked_add(events)
            .ok_or(SourceDescriptionFallback::SourceLimit)?;
        self.plan.extension_name_bytes = self
            .plan
            .extension_name_bytes
            .checked_add(name_bytes)
            .ok_or(SourceDescriptionFallback::SourceLimit)?;
        if self.plan.extension_events > MAX_EXTENSION_EVENTS {
            return Err(SourceDescriptionFallback::SourceLimit);
        }
        self.check_retained()
    }

    fn check_retained(&self) -> Result<()> {
        let total = self
            .plan
            .extension_events
            .checked_mul(128)
            .and_then(|bytes| bytes.checked_add(self.plan.extension_name_bytes))
            .and_then(|bytes| bytes.checked_add(self.retained_bytes))
            .ok_or(SourceDescriptionFallback::SourceLimit)?;
        if total > MAX_RETAINED_BYTES {
            Err(SourceDescriptionFallback::SourceLimit)
        } else {
            Ok(())
        }
    }

    fn reference(
        &mut self,
        node: Node<'_>,
        kind: SourceReferenceKind,
        literal: Option<String>,
    ) -> Result<()> {
        self.budget.check()?;
        if self.plan.references.len() >= MAX_REFERENCES {
            return Err(SourceDescriptionFallback::SourceLimit);
        }
        self.retain(literal.as_ref().map_or(0, String::len))?;
        self.plan.references.push(SourceReference {
            basis: SourceReferenceBasis::SourceSyntax,
            kind,
            evidence: crate::span(node, SourceId(0)),
            literal,
        });
        Ok(())
    }
}
