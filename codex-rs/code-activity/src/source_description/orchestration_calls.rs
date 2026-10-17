//! Direct awaited invocation spelling and complete literal option objects.

use super::Invocation;
use super::MAX_ITEMS;
use super::Reader;
use super::Result;
use super::SourceDescriptionFallback;
use super::SourceReferenceKind;
use super::UNSUPPORTED;
use super::ValidatedPoll;
use super::syntax;
use super::values::JoinKind;
use super::values::JoinedResults;
use super::values::ResultKinds;
use super::values::ToolResultKind;
use super::values::ValueOrigin;
use crate::SourceId;
use crate::source_description::patch::LiteralPatchInvocation;
use std::collections::BTreeSet;
use tree_sitter::Node;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum OptionKey {
    Command,
    Workdir,
    OutputTokens,
    YieldMilliseconds,
    Login,
    Tty,
    Shell,
    SandboxPermissions,
    Justification,
    PrefixRule,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum PollOptionKey {
    SessionId,
    Characters,
    YieldMilliseconds,
    OutputTokens,
}

impl PollOptionKey {
    fn parse(key: &str) -> Result<Self> {
        match key {
            "session_id" => Ok(Self::SessionId),
            "chars" => Ok(Self::Characters),
            "yield_time_ms" => Ok(Self::YieldMilliseconds),
            "max_output_tokens" => Ok(Self::OutputTokens),
            _ => Err(UNSUPPORTED),
        }
    }
}

impl OptionKey {
    fn parse(key: &str) -> Result<Self> {
        match key {
            "cmd" => Ok(Self::Command),
            "workdir" => Ok(Self::Workdir),
            "max_output_tokens" => Ok(Self::OutputTokens),
            "yield_time_ms" => Ok(Self::YieldMilliseconds),
            "login" => Ok(Self::Login),
            "tty" => Ok(Self::Tty),
            "shell" => Ok(Self::Shell),
            "sandbox_permissions" => Ok(Self::SandboxPermissions),
            "justification" => Ok(Self::Justification),
            "prefix_rule" => Ok(Self::PrefixRule),
            _ => Err(UNSUPPORTED),
        }
    }
}

impl Reader<'_> {
    pub(super) fn awaited(&mut self, node: Node<'_>) -> Result<ValueOrigin> {
        if self.groups >= MAX_ITEMS {
            return Err(SourceDescriptionFallback::SourceLimit);
        }
        if node.kind() != "await_expression" {
            return Err(UNSUPPORTED);
        }
        syntax::punctuation(node, &["await"], self.budget)?;
        let parts = syntax::parts(node, self.budget)?;
        let [call] = parts.as_slice() else {
            return Err(UNSUPPORTED);
        };
        let (function, arguments) = syntax::call(*call, self.budget)?;
        let (receiver, method) = syntax::member(function, self.source, self.budget)?;
        let shape = match (receiver, method) {
            ("tools", "exec_command" | "apply_patch" | "write_stdin") => {
                ValueOrigin::Result(ResultKinds::one(self.invocation(*call)?))
            }
            ("Promise", "all" | "allSettled") => {
                let arguments = syntax::sequence(arguments, "(", ")", self.budget)?;
                let [array] = arguments.as_slice() else {
                    return Err(UNSUPPORTED);
                };
                if array.kind() != "array" {
                    return Err(UNSUPPORTED);
                }
                let kind = if method == "all" {
                    JoinKind::All
                } else {
                    JoinKind::AllSettled
                };
                let mut joined = JoinedResults::new(kind);
                for child in syntax::sequence(*array, "[", "]", self.budget)? {
                    joined.push(self.invocation(child)?)?;
                }
                ValueOrigin::Joined(joined)
            }
            _ => return Err(UNSUPPORTED),
        };
        self.groups += 1;
        Ok(shape)
    }

    fn invocation(&mut self, node: Node<'_>) -> Result<ToolResultKind> {
        if self.plan.invocations.len() >= MAX_ITEMS {
            return Err(SourceDescriptionFallback::SourceLimit);
        }
        let (function, arguments) = syntax::call(node, self.budget)?;
        let member = syntax::member(function, self.source, self.budget)?;
        let arguments = syntax::sequence(arguments, "(", ")", self.budget)?;
        let [argument] = arguments.as_slice() else {
            return Err(UNSUPPORTED);
        };
        let (invocation, kind) = match member {
            ("tools", "exec_command") if argument.kind() == "object" => {
                self.reference(node, SourceReferenceKind::InvocationCall, None)?;
                let command = self.command_options(*argument)?;
                self.retain(command.len())?;
                (
                    Invocation::Command {
                        command,
                        evidence: crate::span(node, SourceId(0)),
                    },
                    ToolResultKind::Command,
                )
            }
            ("tools", "apply_patch") => (self.patch(node, *argument)?, ToolResultKind::Patch),
            ("tools", "write_stdin") if argument.kind() == "object" => {
                self.extension(1, 0)?;
                self.reference(node, SourceReferenceKind::InvocationCall, None)?;
                self.poll_options(*argument)?;
                (Invocation::Poll(ValidatedPoll), ToolResultKind::Poll)
            }
            _ => return Err(UNSUPPORTED),
        };
        self.plan.invocations.push(invocation);
        Ok(kind)
    }

    fn poll_options(&mut self, node: Node<'_>) -> Result<()> {
        let mut seen = BTreeSet::new();
        for pair in syntax::sequence(node, "{", "}", self.budget)? {
            if pair.kind() != "pair" || syntax::parts(pair, self.budget)?.len() != 2 {
                return Err(UNSUPPORTED);
            }
            syntax::punctuation(pair, &[":"], self.budget)?;
            let key = syntax::field(pair, "key")?;
            let value = syntax::field(pair, "value")?;
            let key = match key.kind() {
                "property_identifier" => PollOptionKey::parse(crate::text(key, self.source))?,
                "string" => PollOptionKey::parse(&syntax::decode(
                    crate::text(key, self.source),
                    self.budget,
                )?)?,
                _ => return Err(UNSUPPORTED),
            };
            if !seen.insert(key) {
                return Err(UNSUPPORTED);
            }
            let literal = match key {
                PollOptionKey::SessionId => syntax::signed_decimal(
                    value,
                    self.source,
                    2_147_483_647,
                    2_147_483_648,
                    self.budget,
                )?,
                PollOptionKey::Characters => {
                    let value = self.option_literal(value)?;
                    if !value.is_empty() {
                        return Err(UNSUPPORTED);
                    }
                    value
                }
                PollOptionKey::YieldMilliseconds | PollOptionKey::OutputTokens => {
                    syntax::decimal(value, self.source)?
                }
            };
            self.extension(1, 0)?;
            self.reference(pair, SourceReferenceKind::CommandOption, Some(literal))?;
        }
        if seen.contains(&PollOptionKey::SessionId) {
            Ok(())
        } else {
            Err(UNSUPPORTED)
        }
    }

    fn patch(&mut self, node: Node<'_>, argument: Node<'_>) -> Result<Invocation> {
        // This checks the literal node kind before the existing decoder can
        // resolve text bindings. Patch arguments never accept aliases.
        if !matches!(argument.kind(), "string" | "template_string") {
            return Err(UNSUPPORTED);
        }
        let payload = self.string(argument)?;
        self.retain(payload.len())?;
        self.reference(node, SourceReferenceKind::InvocationCall, None)?;
        Ok(Invocation::Patch(LiteralPatchInvocation {
            payload,
            invocation: crate::span(node, SourceId(0)),
            argument: crate::span(argument, SourceId(0)),
        }))
    }

    fn command_options(&mut self, node: Node<'_>) -> Result<String> {
        let mut seen = BTreeSet::new();
        let mut command = None;
        for pair in syntax::sequence(node, "{", "}", self.budget)? {
            if pair.kind() != "pair" || syntax::parts(pair, self.budget)?.len() != 2 {
                return Err(UNSUPPORTED);
            }
            syntax::punctuation(pair, &[":"], self.budget)?;
            let key = syntax::field(pair, "key")?;
            let value = syntax::field(pair, "value")?;
            let key = match key.kind() {
                "property_identifier" => OptionKey::parse(crate::text(key, self.source))?,
                "string" => {
                    OptionKey::parse(&syntax::decode(crate::text(key, self.source), self.budget)?)?
                }
                _ => return Err(UNSUPPORTED),
            };
            if !seen.insert(key) {
                return Err(UNSUPPORTED);
            }
            match key {
                OptionKey::Command => command = Some(self.string(value)?),
                OptionKey::Workdir => {
                    let literal = self.string(value)?;
                    self.reference(pair, SourceReferenceKind::CommandOption, Some(literal))?;
                }
                OptionKey::OutputTokens | OptionKey::YieldMilliseconds => {
                    let literal = syntax::decimal(value, self.source)?;
                    self.reference(pair, SourceReferenceKind::CommandOption, Some(literal))?;
                }
                OptionKey::Login | OptionKey::Tty => {
                    let literal = match value.kind() {
                        "true" => "true",
                        "false" => "false",
                        _ => return Err(UNSUPPORTED),
                    };
                    if value.child_count() != 0 || crate::text(value, self.source) != literal {
                        return Err(UNSUPPORTED);
                    }
                    self.reference(
                        pair,
                        SourceReferenceKind::CommandOption,
                        Some(literal.into()),
                    )?;
                }
                OptionKey::Shell | OptionKey::SandboxPermissions | OptionKey::Justification => {
                    let literal = self.option_literal(value)?;
                    if (key == OptionKey::Shell
                        && !matches!(
                            literal.as_str(),
                            "bash" | "sh" | "zsh" | "/bin/bash" | "/bin/sh" | "/bin/zsh"
                        ))
                        || (key == OptionKey::SandboxPermissions
                            && !matches!(literal.as_str(), "use_default" | "require_escalated"))
                    {
                        return Err(UNSUPPORTED);
                    }
                    self.reference(pair, SourceReferenceKind::CommandOption, Some(literal))?;
                }
                OptionKey::PrefixRule => {
                    if value.kind() != "array" {
                        return Err(UNSUPPORTED);
                    }
                    for item in syntax::sequence(value, "[", "]", self.budget)? {
                        self.option_literal(item)?;
                    }
                    // Describe the source pair only; never turn it into an approval capability.
                    self.reference(pair, SourceReferenceKind::CommandOption, None)?;
                }
            }
        }
        command.ok_or(UNSUPPORTED)
    }

    fn option_literal(&self, node: Node<'_>) -> Result<String> {
        match node.kind() {
            "string" | "template_string" => self.string(node),
            _ => Err(UNSUPPORTED),
        }
    }
}
