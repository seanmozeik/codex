//! Ordered carrier reading after whole-module validation.

mod expressions;

use super::super::SelectiveEditHostProfile;
use super::CarrierBarrier;
use super::CarrierEvent;
use super::CarrierPlan;
use super::MAX_ITEMS;
use super::OrchestrationPlan;
use super::Result;
use super::UNSUPPORTED;
use super::syntax::Syntax;
use crate::SourceId;
use crate::source_description::DescriptionBudget;
use crate::source_description::SourceDescriptionFallback;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use tree_sitter::Node;
use tree_sitter::Parser;

const MAX_RETAINED: usize = 1_048_576;

pub(crate) fn extract(
    parser: &mut Parser,
    source: &str,
    source_id: SourceId,
    cwd: Option<&str>,
    budget: DescriptionBudget,
    host_profile: Option<SelectiveEditHostProfile>,
) -> Result<OrchestrationPlan> {
    let tree = crate::source_description::parse(parser, source, budget)?;
    let syntax = Syntax { source, budget };
    let statements = syntax.module(tree.root_node())?;
    let mut reader = Reader {
        syntax,
        host_profile,
        source_id,
        cwd,
        literals: BTreeMap::new(),
        pure_bindings: BTreeSet::new(),
        plan: CarrierPlan { events: vec![] },
        retained: 0,
    };
    for statement in statements {
        reader.statement(statement)?;
    }
    budget.check()?;
    Ok(reader.plan)
}

struct Reader<'source, 'cwd> {
    syntax: Syntax<'source>,
    host_profile: Option<SelectiveEditHostProfile>,
    source_id: SourceId,
    cwd: Option<&'cwd str>,
    literals: BTreeMap<String, String>,
    pure_bindings: BTreeSet<String>,
    plan: CarrierPlan,
    retained: usize,
}

impl Reader<'_, '_> {
    fn statement(&mut self, statement: Node<'_>) -> Result<()> {
        self.syntax.budget.check()?;
        let (value, declaration) = match statement.kind() {
            "empty_statement" => return Ok(()),
            "statement_block" | "for_statement" | "if_statement" => {
                return self.push(CarrierEvent::Opaque {
                    evidence: crate::span(statement, self.source_id),
                    reason: CarrierBarrier::UnsupportedControl,
                });
            }
            "lexical_declaration" => {
                let (name, value, immutable) = self.syntax.declaration(statement)?;
                (value, Some((name.to_owned(), immutable)))
            }
            "expression_statement" => (self.syntax.statement_expression(statement)?, None),
            _ => return Err(UNSUPPORTED),
        };
        let value = self.syntax.peel(value)?;
        if self.pure(value)? {
            if let Some((name, true)) = declaration {
                let text = self.text(value)?;
                let mut bytes = name
                    .len()
                    .checked_add(256)
                    .ok_or(SourceDescriptionFallback::SourceLimit)?;
                if let Some(text) = &text {
                    bytes = bytes
                        .checked_add(name.len())
                        .and_then(|bytes| bytes.checked_add(text.len()))
                        .ok_or(SourceDescriptionFallback::SourceLimit)?;
                }
                self.retain(bytes)?;
                if let Some(text) = text {
                    self.literals.insert(name.clone(), text);
                }
                self.pure_bindings.insert(name);
            }
            return Ok(());
        }
        if let Some(event) = self.display_argument(value)? {
            // Only the explicit host profile models this exact patch consumer.
            // Generic displays and other tool results remain opaque.
            let patch_display = matches!(
                self.host_profile,
                Some(SelectiveEditHostProfile::FreshBuiltinCodeModePatchDisplayV1)
            ) && matches!(&event, CarrierEvent::Patch { .. });
            self.push(event)?;
            if patch_display {
                return self.push(CarrierEvent::PatchDisplayCompleted {
                    evidence: crate::span(statement, self.source_id),
                });
            }
            return self.push(CarrierEvent::Opaque {
                evidence: crate::span(statement, self.source_id),
                reason: CarrierBarrier::UnknownExecution,
            });
        }
        let event = if value.kind() == "await_expression" {
            let children = self.syntax.parts(value)?;
            let [call] = children.as_slice() else {
                return Err(UNSUPPORTED);
            };
            self.invocation(self.syntax.peel(*call)?)?
        } else {
            None
        };
        match event {
            Some(event) => self.push(event),
            None => self.push(CarrierEvent::Opaque {
                evidence: crate::span(statement, self.source_id),
                reason: self.reason(value)?,
            }),
        }
    }

    fn retain(&mut self, bytes: usize) -> Result<()> {
        self.retained = self
            .retained
            .checked_add(bytes)
            .ok_or(SourceDescriptionFallback::SourceLimit)?;
        if self.retained > MAX_RETAINED {
            return Err(SourceDescriptionFallback::SourceLimit);
        }
        Ok(())
    }

    fn push(&mut self, event: CarrierEvent) -> Result<()> {
        if self.plan.events.len() >= MAX_ITEMS {
            return Err(SourceDescriptionFallback::SourceLimit);
        }
        let bytes = match &event {
            CarrierEvent::Child { text, cwd, .. } => {
                text.len().checked_add(cwd.as_ref().map_or(0, String::len))
            }
            CarrierEvent::Patch { payload, cwd, .. } => payload
                .len()
                .checked_add(cwd.as_ref().map_or(0, String::len)),
            CarrierEvent::Opaque { .. } | CarrierEvent::PatchDisplayCompleted { .. } => Some(0),
        }
        .and_then(|bytes| bytes.checked_add(256))
        .ok_or(SourceDescriptionFallback::SourceLimit)?;
        self.retain(bytes)?;
        self.plan.events.push(event);
        Ok(())
    }
}
