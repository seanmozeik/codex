//! Conservative shell execution contexts and redirect ownership.
use super::Shell;
use crate::children;
use crate::text;
use tree_sitter::Node;

pub(super) enum CommandContext {
    DirectoryHandled,
    Ordinary,
    UnverifiedStartup,
}

impl Shell<'_> {
    pub(super) fn context_gap(&mut self, node: Node<'_>, startup_unknown: bool) {
        if startup_unknown || self.carriers_unknown {
            self.gap(node, "Executable identity or startup context is unresolved");
        }
        if startup_unknown {
            self.gap(
                node,
                "Inline assignments change interpreter runtime context",
            );
        }
    }

    pub(super) fn visit_conditional(
        &mut self,
        node: Node<'_>,
        level: usize,
        heredoc: Option<(usize, Node<'_>)>,
    ) {
        self.gap(node, "Shell condition or pipeline can prevent execution");
        let incoming = self.cwd.clone();
        let identities = self.carriers_unknown;
        let pipeline = node.kind() == "pipeline";
        let mut joined = incoming.clone();
        for child in children(node) {
            if pipeline {
                self.cwd = incoming.clone();
                self.carriers_unknown = identities;
            }
            self.visit(child, level + 1, heredoc);
            if !pipeline && joined != self.cwd {
                joined = None;
            }
            if !pipeline {
                self.cwd.clone_from(&joined);
            }
        }
        if pipeline {
            // Some shells run the final stage in the caller's context.
            self.cwd = if self.cwd == incoming { incoming } else { None };
            self.carriers_unknown |= identities;
        }
    }

    pub(super) fn command_context(
        &mut self,
        node: Node<'_>,
        name: &str,
        values: &[Option<String>],
    ) -> CommandContext {
        let assignments = children(node)
            .iter()
            .any(|child| child.kind() == "variable_assignment");
        if name == "cd" {
            if assignments || self.carriers_unknown {
                self.cwd = None;
                self.gap(node, "Directory execution context is unresolved");
            } else {
                self.change_directory(node, values);
            }
            return CommandContext::DirectoryHandled;
        }
        if mutates_shell(name)
            || matches!(name, "command" | "builtin")
                && forwarded_builtin(values).is_some_and(mutates_shell)
        {
            self.carriers_unknown = true;
            self.cwd = None;
        }
        if assignments {
            CommandContext::UnverifiedStartup
        } else {
            CommandContext::Ordinary
        }
    }

    pub(super) fn change_directory(&mut self, node: Node<'_>, words: &[Option<String>]) {
        let operands = if words.first().and_then(Option::as_deref) == Some("--") {
            &words[1..]
        } else {
            words
        };
        self.cwd = match operands {
            [Some(path)] if !path.is_empty() && !path.starts_with('-') => {
                if path.starts_with('/') {
                    Some(path.clone())
                } else {
                    self.cwd.as_ref().map(|base| format!("{base}/{path}"))
                }
            }
            _ => {
                self.gap(
                    node,
                    "Directory options, operands or expansion are unresolved",
                );
                None
            }
        };
        self.gap(node, "Working directory is conditional on cd succeeding");
    }

    pub(super) fn inspect_redirects(&mut self, root: Node<'_>) {
        let mut pending = vec![(root, 0)];
        let mut inputs = 0;
        let mut files = false;
        while let Some((node, depth)) = pending.pop() {
            if depth > crate::MAX_NESTING {
                self.input_uncertain = true;
                self.gap(root, "Redirect traversal budget exceeded");
                return;
            }
            if node.kind() == "heredoc_body" {
                continue;
            }
            let stdin = node
                .child_by_field_name("descriptor")
                .is_none_or(|descriptor| text(descriptor, self.source) == "0");
            match node.kind() {
                "file_redirect" => {
                    files = true;
                    let mut cursor = node.walk();
                    if stdin
                        && node
                            .children(&mut cursor)
                            .any(|child| matches!(child.kind(), "<" | "<&" | "<&-"))
                    {
                        inputs += 1;
                    }
                }
                "heredoc_redirect" | "herestring_redirect" => {
                    if stdin {
                        inputs += 1;
                    } else {
                        self.input_uncertain = true;
                        self.gap(node, "Literal input redirect does not own standard input");
                    }
                }
                _ => {}
            }
            pending.extend(children(node).into_iter().map(|child| (child, depth + 1)));
        }
        if files {
            self.gap(root, "Redirection effects are not analysed");
        }
        if inputs > 1 {
            self.input_uncertain = true;
            self.gap(root, "Competing standard input redirects are unresolved");
        }
    }
}

fn mutates_shell(name: &str) -> bool {
    matches!(
        name,
        "alias"
            | "unalias"
            | "source"
            | "."
            | "eval"
            | "enable"
            | "shopt"
            | "cd"
            | "pushd"
            | "popd"
    )
}

fn forwarded_builtin(values: &[Option<String>]) -> Option<&str> {
    let mut operands = values.iter().map(Option::as_deref);
    loop {
        match operands.next()? {
            Some("-p" | "--") => {}
            Some(name) if !name.starts_with('-') => return Some(name),
            _ => return None,
        }
    }
}
