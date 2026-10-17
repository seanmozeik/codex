//! Sequential, explicitly owned shell carriers for selective edit forecasts.
//!
//! Ordinary commands are opaque regardless of the source-description whitelist.
//! The caller must serialize the conditional standard dispatch/startup assumption.

mod command;
mod syntax;

use super::input::SourceKind;
use super::orchestration::CarrierBarrier;
use super::orchestration::CarrierEvent;
use super::orchestration::CarrierPlan;
use crate::Language;
use crate::SourceId;
use crate::Span;
use crate::shell::input;
use crate::source_description::DescriptionBudget;
use crate::source_description::SourceDescriptionFallback;
use tree_sitter::Node;
use tree_sitter::Parser;

type Result<T> = std::result::Result<T, SourceDescriptionFallback>;
const UNSUPPORTED: SourceDescriptionFallback = SourceDescriptionFallback::UnsupportedSource;
const MAX_ITEMS: usize = 256;
const MAX_TEXT: usize = 16_320;
const MAX_RETAINED: usize = 1_048_576;

pub(crate) type ShellPlan = CarrierPlan;

pub(crate) fn extract(
    parser: &mut Parser,
    source: &str,
    source_id: SourceId,
    cwd: Option<&str>,
    budget: DescriptionBudget,
) -> Result<ShellPlan> {
    let tree = crate::source_description::parse(parser, source, budget)?;
    let root = tree.root_node();
    if root.kind() != "program" {
        return Err(UNSUPPORTED);
    }
    let mut reader = Reader {
        source,
        source_id,
        cwd,
        budget,
        plan: CarrierPlan { events: vec![] },
        retained: 0,
    };
    let parts = reader.parts(root)?;
    let mut index = 0;
    while let Some(part) = parts.get(index) {
        budget.check()?;
        if !part.node.is_named() && matches!(part.node.kind(), ";" | "\n") {
            index += 1;
            continue;
        }
        if let Some(next) = parts.get(index + 1)
            && !next.node.is_named()
            && next.node.kind() == "&"
        {
            let evidence = reader.range(part.node.start_byte(), next.node.end_byte())?;
            reader.push(CarrierEvent::Opaque {
                evidence,
                reason: CarrierBarrier::ConcurrentExecution,
            })?;
            index += 2;
            continue;
        }
        let event = reader.statement(part.node)?;
        reader.push(event)?;
        index += 1;
    }
    budget.check()?;
    Ok(reader.plan)
}

struct Part<'tree> {
    node: Node<'tree>,
    field: Option<&'static str>,
}

struct Word {
    value: String,
    evidence: Span,
}

struct Reader<'source, 'cwd> {
    source: &'source str,
    source_id: SourceId,
    cwd: Option<&'cwd str>,
    budget: DescriptionBudget,
    plan: CarrierPlan,
    retained: usize,
}

impl Reader<'_, '_> {
    fn statement(&self, node: Node<'_>) -> Result<CarrierEvent> {
        self.budget.check()?;
        if !matches!(node.kind(), "command" | "redirected_statement") {
            return Ok(CarrierEvent::Opaque {
                evidence: crate::span(node, self.source_id),
                reason: if matches!(
                    node.kind(),
                    "variable_assignment"
                        | "variable_assignments"
                        | "declaration_command"
                        | "unset_command"
                ) {
                    CarrierBarrier::ContextMutation
                } else if node.kind() == "&" {
                    CarrierBarrier::ConcurrentExecution
                } else {
                    CarrierBarrier::UnsupportedControl
                },
            });
        }
        match self.carrier(node) {
            Ok(event) => Ok(event),
            Err(SourceDescriptionFallback::UnsupportedSource) => Ok(CarrierEvent::Opaque {
                evidence: crate::span(node, self.source_id),
                reason: self.barrier(node)?,
            }),
            Err(reason) => Err(reason),
        }
    }

    fn carrier(&self, statement: Node<'_>) -> Result<CarrierEvent> {
        let (command, mut doc) = self.owner(statement)?;
        let mut words = Vec::new();
        let mut previous = command.start_byte();
        for part in self.parts(command)? {
            self.budget.check()?;
            match (part.field, part.node.kind()) {
                (Some("name"), "command_name") if words.is_empty() => {
                    self.horizontal(previous, part.node.start_byte())?;
                    let names = self.parts(part.node)?;
                    let [name] = names.as_slice() else {
                        return Err(UNSUPPORTED);
                    };
                    if name.field.is_some() {
                        return Err(UNSUPPORTED);
                    }
                    words.push(Word {
                        value: self.literal(name.node)?,
                        evidence: crate::span(part.node, self.source_id),
                    });
                    previous = part.node.end_byte();
                }
                (Some("argument"), _)
                    if !words.is_empty()
                        && doc.is_none_or(|doc| part.node.end_byte() <= doc.start_byte()) =>
                {
                    self.horizontal(previous, part.node.start_byte())?;
                    words.push(Word {
                        value: self.literal(part.node)?,
                        evidence: crate::span(part.node, self.source_id),
                    });
                    previous = part.node.end_byte();
                }
                (Some("redirect"), "heredoc_redirect") if doc.is_none() => doc = Some(part.node),
                _ => return Err(UNSUPPORTED),
            }
        }
        if words.is_empty() {
            return Err(UNSUPPORTED);
        }
        if let Some(doc) = doc {
            if doc.start_byte() < previous {
                return Err(UNSUPPORTED);
            }
            let gap = self
                .source
                .get(previous..doc.start_byte())
                .ok_or(UNSUPPORTED)?;
            if gap.trim_matches([' ', '\t']) == "-" {
                // This exact missing-token repair is confined to owned stdin.
                // No other bytes may disappear between argv and the heredoc.
                let offset = gap.find('-').ok_or(UNSUPPORTED)?;
                let start = previous.checked_add(offset).ok_or(UNSUPPORTED)?;
                self.horizontal(previous, start)?;
                self.horizontal(start.checked_add(1).ok_or(UNSUPPORTED)?, doc.start_byte())?;
                words.push(Word {
                    value: "-".into(),
                    evidence: self.range(start, start + 1)?,
                });
            } else {
                self.horizontal(previous, doc.start_byte())?;
            }
            if command.end_byte() > previous && command.end_byte() < doc.end_byte() {
                self.horizontal(previous, command.end_byte())?;
            }
            if statement.end_byte() < doc.end_byte() {
                return Err(UNSUPPORTED);
            }
            self.trivia(doc.end_byte(), statement.end_byte())?;
        } else {
            self.horizontal(previous, command.end_byte())?;
            if statement != command {
                return Err(UNSUPPORTED);
            }
        }

        let executable = &words[0].value;
        let Some(language) = input::language(executable) else {
            return Err(UNSUPPORTED);
        };
        let kind = match language {
            Language::Python => SourceKind::Python,
            Language::Shell => SourceKind::Shell,
            Language::TypeScript
                if matches!(executable.rsplit('/').next(), Some("node" | "nodejs")) =>
            {
                SourceKind::NodeCommonJs
            }
            Language::TypeScript => return Err(UNSUPPORTED),
        };
        self.argv_gate(&words, language)?;
        // A wrapper can alter cwd, environment, dispatch, or startup behavior.
        // Only the interpreter's own literal argv reaches the shared resolver.
        let argv: Vec<_> = words.iter().map(|word| Some(word.value.clone())).collect();
        let invocation = input::resolve(&argv).ok_or(UNSUPPORTED)?;
        if invocation.context_unknown || invocation.language != language {
            return Err(UNSUPPORTED);
        }
        let (text, evidence) = match invocation.input {
            input::Input::Inline { index, prefix } if doc.is_none() => {
                let word = words.get(index).ok_or(UNSUPPORTED)?;
                (
                    word.value.get(prefix..).ok_or(UNSUPPORTED)?.to_owned(),
                    word.evidence.clone(),
                )
            }
            input::Input::Stdin => {
                let evidence = self.heredoc(doc.ok_or(UNSUPPORTED)?)?;
                (
                    self.source
                        .get(evidence.start_byte..evidence.end_byte)
                        .ok_or(UNSUPPORTED)?
                        .to_owned(),
                    evidence,
                )
            }
            _ => return Err(UNSUPPORTED),
        };
        if text.len() > MAX_TEXT {
            return Err(SourceDescriptionFallback::SourceLimit);
        }
        Ok(CarrierEvent::Child {
            kind,
            text,
            evidence,
            cwd: self.cwd.map(str::to_owned),
            invocation: crate::span(statement, self.source_id),
        })
    }

    fn owner<'tree>(&self, node: Node<'tree>) -> Result<(Node<'tree>, Option<Node<'tree>>)> {
        if node.kind() == "command" {
            return Ok((node, None));
        }
        if node.kind() != "redirected_statement" {
            return Err(UNSUPPORTED);
        }
        let mut body = None;
        let mut doc = None;
        for part in self.parts(node)? {
            match (part.field, part.node.kind()) {
                (Some("body"), "command") if body.is_none() => body = Some(part.node),
                (Some("redirect"), "heredoc_redirect") if doc.is_none() => doc = Some(part.node),
                _ => return Err(UNSUPPORTED),
            }
        }
        let body = body.ok_or(UNSUPPORTED)?;
        self.horizontal(node.start_byte(), body.start_byte())?;
        Ok((body, Some(doc.ok_or(UNSUPPORTED)?)))
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
        self.retained = self
            .retained
            .checked_add(bytes)
            .ok_or(SourceDescriptionFallback::SourceLimit)?;
        if self.retained > MAX_RETAINED {
            return Err(SourceDescriptionFallback::SourceLimit);
        }
        self.plan.events.push(event);
        Ok(())
    }
}
