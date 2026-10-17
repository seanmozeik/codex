use super::Result;
use super::SourceReferenceKind;
use super::State;
use super::UNSUPPORTED;
use super::literals;
use crate::SourceId;
use tree_sitter::Node;

impl State<'_> {
    pub(super) fn sequence(&mut self, parts: &[literals::Part<'_>]) -> Result<usize> {
        let mut statements = 0;
        for part in parts {
            self.budget.check()?;
            match part.node.kind() {
                ";" | "\n" if !part.node.is_named() => {}
                _ => {
                    self.statement(part.node)?;
                    statements += 1;
                }
            }
        }
        Ok(statements)
    }

    fn statement(&mut self, node: Node<'_>) -> Result<()> {
        self.budget.check()?;
        match node.kind() {
            "command" => self.atom(node, true),
            "redirected_statement" => self.redirected_statement(node),
            "list" => {
                let parts = literals::parts(node, self.budget)?;
                let [left, operator, right] = parts.as_slice() else {
                    return Err(UNSUPPORTED);
                };
                if operator.node.is_named() || !matches!(operator.node.kind(), "&&" | "||") {
                    return Err(UNSUPPORTED);
                }
                self.event()?;
                self.statement(left.node)?;
                self.connector(operator.node)?;
                self.statement(right.node)
            }
            "pipeline" => {
                let parts = literals::parts(node, self.budget)?;
                if parts.len() < 3 || parts.len() % 2 != 1 {
                    return Err(UNSUPPORTED);
                }
                self.event()?;
                self.statement(parts.first().ok_or(UNSUPPORTED)?.node)?;
                for pair in parts.get(1..).ok_or(UNSUPPORTED)?.chunks_exact(2) {
                    let [operator, right] = pair else {
                        return Err(UNSUPPORTED);
                    };
                    if operator.node.is_named() || operator.node.kind() != "|" {
                        return Err(UNSUPPORTED);
                    }
                    self.connector(operator.node)?;
                    self.statement(right.node)?;
                }
                Ok(())
            }
            "subshell" | "compound_statement" => self.group(node),
            _ => Err(UNSUPPORTED),
        }
    }

    fn redirected_statement(&mut self, node: Node<'_>) -> Result<()> {
        let Some(body) = node
            .child_by_field_name("body")
            .filter(|body| matches!(body.kind(), "list" | "pipeline"))
        else {
            return self.atom(node, true);
        };
        let parts = literals::parts(node, self.budget)?;
        let (head, tail) = parts.split_first().ok_or(UNSUPPORTED)?;
        if head.field != Some("body")
            || head.node != body
            || tail.is_empty()
            || tail.iter().any(|part| {
                part.field != Some("redirect")
                    || !matches!(part.node.kind(), "file_redirect" | "heredoc_redirect")
                    || part.node.start_byte() < body.end_byte()
            })
            || tail
                .iter()
                .filter(|part| part.node.kind() == "heredoc_redirect")
                .count()
                > 1
        {
            return Err(UNSUPPORTED);
        }
        // This pinned wrapper records trailing redirects outside its list or
        // pipeline. Their owner is the exact final atom, never a chosen descendant.
        self.statement_with_redirects(body, tail.iter().map(|part| part.node).collect())
    }

    fn statement_with_redirects<'tree>(
        &mut self,
        node: Node<'tree>,
        redirects: Vec<Node<'tree>>,
    ) -> Result<()> {
        self.budget.check()?;
        match node.kind() {
            "command" | "redirected_statement" => self.atom_with_redirects(node, true, redirects),
            "list" => {
                let parts = literals::parts(node, self.budget)?;
                let [left, operator, right] = parts.as_slice() else {
                    return Err(UNSUPPORTED);
                };
                if operator.node.is_named() || !matches!(operator.node.kind(), "&&" | "||") {
                    return Err(UNSUPPORTED);
                }
                self.event()?;
                self.statement(left.node)?;
                self.connector(operator.node)?;
                self.statement_with_redirects(right.node, redirects)
            }
            "pipeline" => {
                let parts = literals::parts(node, self.budget)?;
                if parts.len() < 3 || parts.len() % 2 != 1 {
                    return Err(UNSUPPORTED);
                }
                self.event()?;
                self.statement(parts.first().ok_or(UNSUPPORTED)?.node)?;
                let final_pair = parts.len() - 2;
                for pair in parts.get(1..final_pair).ok_or(UNSUPPORTED)?.chunks_exact(2) {
                    let [operator, right] = pair else {
                        return Err(UNSUPPORTED);
                    };
                    if operator.node.is_named() || operator.node.kind() != "|" {
                        return Err(UNSUPPORTED);
                    }
                    self.connector(operator.node)?;
                    self.statement(right.node)?;
                }
                let [operator, right] = parts.get(final_pair..).ok_or(UNSUPPORTED)? else {
                    return Err(UNSUPPORTED);
                };
                if operator.node.is_named() || operator.node.kind() != "|" {
                    return Err(UNSUPPORTED);
                }
                self.connector(operator.node)?;
                self.statement_with_redirects(right.node, redirects)
            }
            _ => Err(UNSUPPORTED),
        }
    }

    fn group(&mut self, node: Node<'_>) -> Result<()> {
        let parts = literals::parts(node, self.budget)?;
        let (open, tail) = parts.split_first().ok_or(UNSUPPORTED)?;
        let (close, body) = tail.split_last().ok_or(UNSUPPORTED)?;
        let delimiters = match node.kind() {
            "subshell" => ("(", ")"),
            "compound_statement" => ("{", "}"),
            _ => return Err(UNSUPPORTED),
        };
        if open.node.is_named()
            || close.node.is_named()
            || open.node.kind() != delimiters.0
            || close.node.kind() != delimiters.1
        {
            return Err(UNSUPPORTED);
        }
        if delimiters.0 == "{"
            && body
                .iter()
                .rev()
                .find(|part| part.node.kind() != "\n")
                .is_none_or(|part| part.node.kind() != ";")
        {
            return Err(UNSUPPORTED);
        }
        self.event()?;
        self.connector(open.node)?;
        if self.sequence(body)? == 0 {
            return Err(UNSUPPORTED);
        }
        self.connector(close.node)
    }

    pub(super) fn connector(&mut self, node: Node<'_>) -> Result<()> {
        self.reference(
            SourceReferenceKind::ShellConnector,
            crate::span(node, SourceId(0)),
            Some(crate::text(node, self.source).to_owned()),
        )
    }
}
