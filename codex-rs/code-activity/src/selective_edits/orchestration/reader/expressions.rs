//! Recognized invocations, literal bindings and opaque-expression reasons.

use super::super::super::input::SourceKind;
use super::super::CarrierBarrier;
use super::super::CarrierEvent;
use super::super::Result;
use super::super::UNSUPPORTED;
use super::Reader;
use tree_sitter::Node;

impl Reader<'_, '_> {
    pub(super) fn display_argument(&self, node: Node<'_>) -> Result<Option<CarrierEvent>> {
        if node.kind() != "call_expression" {
            return Ok(None);
        }
        let (callee, arguments) = self.syntax.call(node)?;
        // Do not descend into arbitrary callees or their arguments. The full
        // module gate rejects lexical text/tools shadowing, including TDZs;
        // the dispatcher stops at any prior opaque mutation or execution.
        if callee.kind() != "identifier" || self.syntax.identifier(callee)? != "text" {
            return Ok(None);
        }
        let [argument] = arguments.as_slice() else {
            return Ok(None);
        };
        let argument = self.syntax.peel(*argument)?;
        if argument.kind() != "await_expression" {
            return Ok(None);
        }
        let children = self.syntax.parts(argument)?;
        let [call] = children.as_slice() else {
            return Err(UNSUPPORTED);
        };
        let call = self.syntax.peel(*call)?;
        if call.kind() != "call_expression" {
            return Ok(None);
        }
        self.invocation(call)
    }

    pub(super) fn invocation(&self, call: Node<'_>) -> Result<Option<CarrierEvent>> {
        let (callee, arguments) = self.syntax.call(call)?;
        if callee.kind() != "member_expression" {
            return Ok(None);
        }
        let (object, method) = self.syntax.member(callee)?;
        if object.kind() != "identifier" || self.syntax.identifier(object)? != "tools" {
            return Ok(None);
        }
        let [argument] = arguments.as_slice() else {
            return Ok(None);
        };
        match method {
            "apply_patch" if matches!(argument.kind(), "string" | "template_string") => {
                Ok(Some(CarrierEvent::Patch {
                    payload: self.syntax.literal_text(*argument)?,
                    invocation: crate::span(call, self.source_id),
                    argument: crate::span(*argument, self.source_id),
                    cwd: self.cwd.map(str::to_owned),
                }))
            }
            "exec_command" if argument.kind() == "object" => self.command(*argument, call),
            _ => Ok(None),
        }
    }

    fn command(&self, object: Node<'_>, call: Node<'_>) -> Result<Option<CarrierEvent>> {
        let mut command = None;
        let mut command_evidence = None;
        let mut cwd = self.cwd.map(str::to_owned);
        for pair in self.syntax.sequence(object, "{", "}")? {
            // Syntax admission does not establish a decoded command option:
            // retain the prefix and stop at opacity.
            if pair.kind() == "shorthand_property_identifier" {
                return Ok(None);
            }
            let key = pair.child_by_field_name("key").ok_or(UNSUPPORTED)?;
            if key.kind() != "property_identifier" {
                return Ok(None);
            }
            let (name, value) = self.syntax.pair(pair)?;
            match name.as_str() {
                "cmd" => {
                    command = self.text(value)?;
                    command_evidence = Some(crate::span(value, self.source_id));
                    if command.as_ref().is_none_or(|value| value.contains('\0')) {
                        return Ok(None);
                    }
                }
                "workdir" => {
                    let Some(value) = self.text(value)? else {
                        return Ok(None);
                    };
                    if !value.starts_with('/') || value.contains('\0') {
                        return Ok(None);
                    }
                    cwd = Some(value);
                }
                "shell" => {
                    let Some(value) = self.text(value)? else {
                        return Ok(None);
                    };
                    if !matches!(
                        value.as_str(),
                        "bash"
                            | "sh"
                            | "zsh"
                            | "/bin/bash"
                            | "/bin/sh"
                            | "/bin/zsh"
                            | "/usr/bin/bash"
                            | "/usr/bin/sh"
                            | "/usr/bin/zsh"
                    ) {
                        return Ok(None);
                    }
                }
                "sandbox_permissions" => {
                    let Some(value) = self.text(value)? else {
                        return Ok(None);
                    };
                    if !matches!(value.as_str(), "use_default" | "require_escalated") {
                        return Ok(None);
                    }
                }
                "justification" => {
                    if self.text(value)?.is_none() {
                        return Ok(None);
                    }
                }
                "max_output_tokens" | "yield_time_ms" => {
                    if value.kind() != "number" {
                        return Ok(None);
                    }
                    self.syntax.decimal(value)?;
                }
                "login" | "tty" => {
                    if !matches!(value.kind(), "true" | "false") {
                        return Ok(None);
                    }
                }
                "prefix_rule" => {
                    if value.kind() != "array" {
                        return Ok(None);
                    }
                    for item in self.syntax.sequence(value, "[", "]")? {
                        if self.text(item)?.is_none() {
                            return Ok(None);
                        }
                    }
                }
                _ => return Ok(None),
            }
        }
        let (Some(text), Some(evidence)) = (command, command_evidence) else {
            return Ok(None);
        };
        Ok(Some(CarrierEvent::Child {
            kind: SourceKind::Shell,
            text,
            evidence,
            cwd,
            invocation: crate::span(call, self.source_id),
        }))
    }

    pub(super) fn text(&self, node: Node<'_>) -> Result<Option<String>> {
        let node = self.syntax.peel(node)?;
        match node.kind() {
            "string" | "template_string" => self.syntax.literal_text(node).map(Some),
            "identifier" => Ok(self.literals.get(self.syntax.identifier(node)?).cloned()),
            _ => Ok(None),
        }
    }

    pub(super) fn pure(&self, node: Node<'_>) -> Result<bool> {
        self.syntax.budget.check()?;
        match node.kind() {
            "string" | "template_string" | "number" | "true" | "false" | "null" => Ok(true),
            "identifier" => Ok(self.pure_bindings.contains(self.syntax.identifier(node)?)),
            "parenthesized_expression" => self.pure(self.syntax.parenthesized(node)?),
            "array" => {
                for value in self.syntax.sequence(node, "[", "]")? {
                    if !self.pure(value)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            "object" => {
                for pair in self.syntax.sequence(node, "{", "}")? {
                    // Admitted syntax is not proof of a pure binding lookup.
                    // Do not turn this opaque suffix into a module fallback.
                    if pair.kind() == "shorthand_property_identifier" {
                        return Ok(false);
                    }
                    if !self.pure(self.syntax.pair(pair)?.1)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    pub(super) fn reason(&self, node: Node<'_>) -> Result<CarrierBarrier> {
        let node = self.syntax.peel(node)?;
        if node.kind() == "assignment_expression" {
            return Ok(CarrierBarrier::ContextMutation);
        }
        let awaited = node.kind() == "await_expression";
        let node = if awaited {
            let parts = self.syntax.parts(node)?;
            let [value] = parts.as_slice() else {
                return Err(UNSUPPORTED);
            };
            self.syntax.peel(*value)?
        } else {
            node
        };
        if node.kind() == "call_expression" {
            let (callee, arguments) = self.syntax.call(node)?;
            if callee.kind() == "member_expression" {
                let (object, method) = self.syntax.member(callee)?;
                if object.kind() == "identifier" {
                    match self.syntax.identifier(object)? {
                        "Promise" if matches!(method, "all" | "allSettled" | "race" | "any") => {
                            return Ok(CarrierBarrier::ConcurrentExecution);
                        }
                        "tools" if matches!(method, "apply_patch" | "exec_command") => {
                            if !awaited {
                                return Ok(CarrierBarrier::ConcurrentExecution);
                            }
                            if method == "exec_command"
                                && let [object] = arguments.as_slice()
                                && object.kind() == "object"
                            {
                                for pair in self.syntax.sequence(*object, "{", "}")? {
                                    if pair.kind() == "shorthand_property_identifier" {
                                        return Ok(CarrierBarrier::UnresolvedArguments);
                                    }
                                    let (key, value) = self.syntax.pair(pair)?;
                                    if key == "workdir"
                                        && let Some(value) = self.text(value)?
                                        && !value.contains('\0')
                                        && !value.starts_with('/')
                                    {
                                        return Ok(CarrierBarrier::ContextMutation);
                                    }
                                }
                            }
                            return Ok(CarrierBarrier::UnresolvedArguments);
                        }
                        "tools" => return Ok(CarrierBarrier::UnresolvedDispatch),
                        _ => {}
                    }
                }
            }
        }
        Ok(CarrierBarrier::UnknownExecution)
    }
}
