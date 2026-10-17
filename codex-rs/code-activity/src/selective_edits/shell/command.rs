//! Interpreter argument ownership and opaque command classification.

use super::Reader;
use super::Result;
use super::UNSUPPORTED;
use super::Word;
use crate::Language;
use crate::SourceDescriptionFallback;
use crate::selective_edits::orchestration::CarrierBarrier;
use crate::shell::input;
use tree_sitter::Node;

impl Reader<'_, '_> {
    pub(super) fn argv_gate(&self, words: &[Word], language: Language) -> Result<()> {
        self.budget.check()?;
        let name = words
            .first()
            .ok_or(UNSUPPORTED)?
            .value
            .rsplit('/')
            .next()
            .ok_or(UNSUPPORTED)?;
        let mut index = 1;
        if language == Language::Python {
            let version = name
                .strip_prefix("python")
                .or_else(|| name.strip_prefix("pypy"))
                .ok_or(UNSUPPORTED)?;
            // The selective Python model is Python 3. A literal Python 2 name
            // contradicts that model and cannot be repaired with an assumption.
            if !version.is_empty() && !version.starts_with('3') {
                return Err(UNSUPPORTED);
            }
            while words.get(index).is_some_and(|word| {
                matches!(
                    word.value.as_str(),
                    "-I" | "-S" | "-E" | "-s" | "-B" | "-u" | "-q"
                )
            }) {
                self.budget.check()?;
                index += 1;
            }
        }
        let tail = words.get(index..).ok_or(UNSUPPORTED)?;
        if language == Language::TypeScript {
            return match tail {
                [] => Ok(()),
                [word] if word.value == "-" => Ok(()),
                [flag, _] if matches!(flag.value.as_str(), "-e" | "--eval") => Ok(()),
                _ => Err(UNSUPPORTED),
            };
        }
        match tail {
            [] => Ok(()),
            [word] if word.value == "-" => Ok(()),
            [flag, _] if flag.value == "-c" => Ok(()),
            [flag, _] if language == Language::Shell && name == "bash" && flag.value == "-lc" => {
                Ok(())
            }
            [word]
                if language == Language::Python
                    && word.value.starts_with("-c")
                    && word.value.len() > 2 =>
            {
                Ok(())
            }
            _ => Err(UNSUPPORTED),
        }
    }

    pub(super) fn barrier(&self, statement: Node<'_>) -> Result<CarrierBarrier> {
        let command = if statement.kind() == "command" {
            statement
        } else {
            let Some(body) = statement.child_by_field_name("body") else {
                return Ok(CarrierBarrier::UnresolvedDispatch);
            };
            if matches!(
                body.kind(),
                "variable_assignment"
                    | "variable_assignments"
                    | "declaration_command"
                    | "unset_command"
            ) {
                return Ok(CarrierBarrier::ContextMutation);
            }
            if body.kind() != "command" {
                return Ok(CarrierBarrier::UnsupportedControl);
            }
            body
        };
        let parts = self.parts(command)?;
        if parts
            .iter()
            .any(|part| part.node.kind() == "variable_assignment")
        {
            return Ok(CarrierBarrier::ContextMutation);
        }
        let Some(name) = command.child_by_field_name("name") else {
            return Ok(CarrierBarrier::UnresolvedDispatch);
        };
        let names = self.parts(name)?;
        let [name] = names.as_slice() else {
            return Ok(CarrierBarrier::UnresolvedDispatch);
        };
        let name = match self.literal(name.node) {
            Ok(name) => name,
            Err(SourceDescriptionFallback::UnsupportedSource) => {
                return Ok(CarrierBarrier::UnresolvedDispatch);
            }
            Err(reason) => return Err(reason),
        };
        if matches!(
            name.as_str(),
            "cd" | "env"
                | "alias"
                | "unalias"
                | "export"
                | "unset"
                | "set"
                | "source"
                | "."
                | "readonly"
                | "declare"
                | "typeset"
                | "local"
        ) {
            return Ok(CarrierBarrier::ContextMutation);
        }
        if matches!(name.as_str(), "command" | "uv") {
            return Ok(CarrierBarrier::UnresolvedDispatch);
        }
        match input::language(&name) {
            Some(Language::Python | Language::Shell) => Ok(CarrierBarrier::UnresolvedArguments),
            Some(Language::TypeScript) => Ok(CarrierBarrier::UnresolvedDispatch),
            None => Ok(CarrierBarrier::UnknownExecution),
        }
    }
}
