use crate::Analyzer;
use crate::Basis;
use crate::Effect;
use crate::MAX_NESTING;
use crate::MAX_OPERATIONS;
use crate::Operation;
use crate::OperationId;
use crate::Report;
use crate::SourceId;
use crate::Target;
use crate::Unresolved;
use crate::children;
use crate::span;
use crate::text;
use context::CommandContext;
use literals::literal;
use tree_sitter::Node;
mod context;
mod input;
mod literals;

pub fn interpreter_source(argv: &[Option<String>]) -> Option<(crate::Language, String)> {
    let invocation = input::resolve(argv)?;
    if invocation.context_unknown {
        return None;
    }
    let input::Input::Inline { index, prefix } = invocation.input else {
        return None;
    };
    Some((
        invocation.language,
        argv.get(index)?.as_ref()?.get(prefix..)?.to_owned(),
    ))
}

pub fn analyze(
    analyzer: &mut Analyzer,
    node: Node<'_>,
    source: &str,
    id: SourceId,
    cwd: Option<String>,
    depth: usize,
    report: &mut Report,
) {
    let mut context = Shell {
        analyzer,
        source,
        id,
        cwd,
        depth,
        report,
        carriers_unknown: false,
        input_uncertain: false,
    };
    context.visit(node, 0, None);
}
struct Shell<'a> {
    analyzer: &'a mut Analyzer,
    source: &'a str,
    id: SourceId,
    cwd: Option<String>,
    depth: usize,
    report: &'a mut Report,
    carriers_unknown: bool,
    input_uncertain: bool,
}
impl Shell<'_> {
    fn gap(&mut self, node: Node<'_>, reason: &str) {
        if self.report.unresolved.len() < MAX_OPERATIONS {
            self.report.unresolved.push(Unresolved {
                evidence: span(node, self.id),
                reason: reason.into(),
            });
        }
    }
    fn emit(&mut self, node: Node<'_>, effect: Effect) {
        if self.report.operations.len() < MAX_OPERATIONS {
            self.report.operations.push(Operation {
                id: OperationId(self.report.operations.len()),
                evidence: span(node, self.id),
                basis: Basis::StaticIntent,
                effect,
            });
        } else {
            self.gap(node, "Operation budget exceeded");
        }
    }
    fn visit(&mut self, node: Node<'_>, level: usize, heredoc: Option<(usize, Node<'_>)>) {
        if level > MAX_NESTING {
            self.gap(node, "Shell traversal budget exceeded");
            return;
        }
        match node.kind() {
            "program" => {
                for child in children(node) {
                    self.visit(child, level + 1, heredoc);
                }
            }
            "list" | "pipeline" => self.visit_conditional(node, level, heredoc),
            "redirected_statement" => {
                let body = node.child_by_field_name("body");
                let saved = self.input_uncertain;
                self.inspect_redirects(node);
                let docs: Vec<_> = children(node)
                    .into_iter()
                    .filter(|n| matches!(n.kind(), "heredoc_redirect" | "herestring_redirect"))
                    .collect();
                if let Some(body) = body {
                    if docs.len() == 1 {
                        let mut stack = vec![body];
                        let mut command = None;
                        while let Some(n) = stack.pop() {
                            if n.kind() == "command" {
                                command = Some(n);
                            } else {
                                let mut c = children(n);
                                c.reverse();
                                stack.extend(c);
                            }
                        }
                        if let Some(command) = command {
                            self.visit(body, level + 1, Some((command.start_byte(), docs[0])));
                        }
                    } else {
                        self.gap(node, "Redirection effects are not analysed");
                        self.visit(body, level + 1, None);
                    }
                }
                self.input_uncertain = saved;
            }
            "command" => {
                let saved = self.input_uncertain;
                self.inspect_redirects(node);
                self.command(
                    node,
                    heredoc
                        .filter(|(pos, _)| *pos == node.start_byte())
                        .map(|(_, n)| n),
                );
                self.input_uncertain = saved;
            }
            "comment" => {}
            _ => {
                self.gap(
                    node,
                    "Unsupported shell region; contents are not interpreted",
                );
                self.cwd = None;
                self.carriers_unknown = true;
            }
        }
    }
    fn command(&mut self, node: Node<'_>, heredoc: Option<Node<'_>>) {
        let Some(name_node) = node.child_by_field_name("name") else {
            return;
        };
        let name = text(name_node, self.source);
        let mut cursor = node.walk();
        let args: Vec<_> = node
            .children_by_field_name("argument", &mut cursor)
            .collect();
        let mut values: Vec<_> = args.iter().map(|n| literal(*n, self.source)).collect();
        // The Bash grammar can omit a trailing stdin marker before a heredoc.
        if heredoc.is_some_and(|doc| {
            self.source
                .get(node.end_byte()..doc.start_byte())
                .is_some_and(|gap| gap.trim() == "-")
        }) {
            values.push(Some("-".into()));
        }
        let startup_unknown = match self.command_context(node, name, &values) {
            CommandContext::DirectoryHandled => return,
            CommandContext::Ordinary => false,
            CommandContext::UnverifiedStartup => true,
        };
        let executable = name.rsplit('/').next().unwrap_or(name);
        let words: Vec<_> = std::iter::once(Some(name.to_owned()))
            .chain(values.iter().cloned())
            .collect();
        if let Some(invocation) =
            input::resolve(&words).filter(|_| !self.carriers_unknown && !startup_unknown)
        {
            let heredoc = heredoc.or_else(|| {
                children(node)
                    .into_iter()
                    .find(|n| matches!(n.kind(), "herestring_redirect" | "heredoc_redirect"))
            });
            let embedded = self.embedded_source(heredoc, &words, &args, invocation.input);
            if let Some((code, parent)) = embedded {
                if invocation.context_unknown {
                    self.gap(
                        node,
                        "Wrapper changes runtime context; target working directory is unresolved",
                    );
                }
                self.analyzer.source(
                    &code,
                    invocation.language,
                    if invocation.context_unknown {
                        None
                    } else {
                        self.cwd.clone()
                    },
                    Some(parent),
                    self.depth + 1,
                    self.report,
                );
            } else {
                self.gap(
                    node,
                    "Interpreter payload is not a static supported literal",
                );
                self.emit(
                    node,
                    Effect::ProcessRun {
                        argv: std::iter::once(Some(name.into())).chain(values).collect(),
                        shell: None,
                    },
                );
            }
        } else if matches!(executable, "cat" | "head" | "tail")
            && !self.carriers_unknown
            && !startup_unknown
            && values.len() == 1
            && values[0].as_ref().is_some_and(|s| !s.starts_with('-'))
        {
            self.emit(
                node,
                Effect::FileRead {
                    target: Target::Literal {
                        path: values[0].clone().unwrap_or_default(),
                        cwd: self.cwd.clone(),
                    },
                },
            );
        } else {
            self.context_gap(node, startup_unknown);
            if heredoc.is_some() {
                self.gap(
                    node,
                    "Heredoc content is data for this command, not executable Python",
                );
            }
            self.emit(
                node,
                Effect::ProcessRun {
                    argv: std::iter::once(Some(name.into())).chain(values).collect(),
                    shell: None,
                },
            );
            self.gap(node, "Command effects are not analysed");
        }
    }
    fn embedded_source(
        &mut self,
        heredoc: Option<Node<'_>>,
        values: &[Option<String>],
        args: &[Node<'_>],
        input: input::Input,
    ) -> Option<(String, crate::Span)> {
        let mut embedded = None;
        if let input::Input::Inline { index, prefix } = input {
            let code = values.get(index)?.as_ref()?.get(prefix..)?.to_owned();
            return Some((code, span(*args.get(index.checked_sub(1)?)?, self.id)));
        }
        if self.input_uncertain {
            return None;
        }
        if let Some(doc) = heredoc {
            if doc.kind() == "herestring_redirect" {
                let value = children(doc).into_iter().last()?;
                return Some((literal(value, self.source)?, span(value, self.id)));
            }
            let start = children(doc)
                .into_iter()
                .find(|n| n.kind() == "heredoc_start");
            let body = children(doc)
                .into_iter()
                .find(|n| n.kind() == "heredoc_body");
            if let (Some(start), Some(body)) = (start, body) {
                let delimiter = text(start, self.source);
                if delimiter.starts_with(['\'', '"'])
                    || !text(body, self.source).contains(['$', '`', '\\'])
                {
                    embedded = Some((text(body, self.source).to_owned(), span(body, self.id)));
                } else {
                    self.gap(
                        doc,
                        "Unquoted heredoc can expand in the shell; payload is unresolved",
                    );
                }
            }
        }

        embedded
    }
}
