use super::Assignment;
use super::Atom;
use super::AtomInput;
use super::Result;
use super::SourceDescriptionFallback;
use super::SourceReferenceKind;
use super::State;
use super::UNSUPPORTED;
use super::Word;
use super::WordOrigin;
use super::environment_name;
use super::literals;
use super::push_assignment;
use super::redirections;
use crate::SourceId;
use tree_sitter::Node;

impl State<'_> {
    pub(super) fn atom(&mut self, node: Node<'_>, allow_heredoc: bool) -> Result<()> {
        self.atom_with_redirects(node, allow_heredoc, Vec::new())
    }

    pub(super) fn atom_with_redirects<'tree>(
        &mut self,
        node: Node<'tree>,
        allow_heredoc: bool,
        redirects: Vec<Node<'tree>>,
    ) -> Result<()> {
        let mut atom = self.read_atom(node, redirects)?;
        if atom.heredoc.is_some() && !allow_heredoc {
            return Err(UNSUPPORTED);
        }
        if atom.heredoc.is_some()
            && atom
                .redirects
                .iter()
                .any(redirections::FileRedirect::targets_stdin)
        {
            return Err(UNSUPPORTED);
        }

        let executable = atom.words.first().ok_or(UNSUPPORTED)?.value.clone();
        let mut child_offset = None;
        if executable == "env"
            && atom
                .words
                .get(1)
                .is_some_and(|word| word.value.contains('='))
        {
            let mut offset = 1;
            while let Some(word) = atom.words.get(offset)
                && word.value.contains('=')
            {
                let (name, _) = word.value.split_once('=').ok_or(UNSUPPORTED)?;
                if !environment_name(name) {
                    return Err(UNSUPPORTED);
                }
                push_assignment(
                    &mut atom.assignments,
                    Assignment {
                        name: name.to_owned(),
                        spelling: word.value.clone(),
                        evidence: word.evidence.clone(),
                    },
                )?;
                offset += 1;
            }
            if offset == atom.words.len() {
                return Err(UNSUPPORTED);
            }
            child_offset = Some(offset);
        }
        let child_words = atom
            .words
            .get(child_offset.unwrap_or(0)..)
            .ok_or(UNSUPPORTED)?;
        let input = self.atom_input(child_words, atom.heredoc.as_ref(), child_offset.is_some())?;
        for word in &atom.words {
            if word.origin == WordOrigin::DecimalArgument {
                self.event()?;
            }
        }

        self.reference(
            SourceReferenceKind::InvocationCall,
            atom.evidence,
            Some(executable),
        )?;
        for assignment in atom.assignments {
            self.event()?;
            self.reference(
                SourceReferenceKind::EnvironmentAssignment,
                assignment.evidence,
                Some(assignment.spelling),
            )?;
        }
        if child_offset.is_some() {
            self.event()?;
            let first = child_words.first().ok_or(UNSUPPORTED)?;
            let last = child_words.last().ok_or(UNSUPPORTED)?;
            self.reference(
                SourceReferenceKind::InvocationCall,
                literals::range(
                    self.source,
                    first.evidence.start_byte,
                    last.evidence.end_byte,
                )?,
                Some(first.value.clone()),
            )?;
        }
        let data = match input {
            AtomInput::Ordinary => None,
            AtomInput::Directory { target, evidence } => {
                self.event()?;
                self.reference(
                    SourceReferenceKind::DirectoryOperand,
                    evidence,
                    Some(target),
                )?;
                None
            }
            AtomInput::Code(child) => {
                self.retain(child.source.len())?;
                if self.plan.children.len() >= 32 {
                    return Err(SourceDescriptionFallback::SourceLimit);
                }
                self.plan.children.push(child);
                None
            }
            AtomInput::LiteralData { evidence } => Some(evidence),
        };
        for redirect in atom.redirects {
            self.event()?;
            let reference = redirect.into_reference();
            self.reference(reference.kind, reference.evidence, reference.literal)?;
        }
        if let Some(evidence) = data {
            self.event()?;
            self.reference(SourceReferenceKind::HeredocData, evidence, None)?;
        }
        if let Some(heredoc) = atom.heredoc
            && let Some(suffix) = heredoc.suffix
        {
            self.event()?;
            self.connector(suffix.operator)?;
            self.atom(suffix.right, false)?;
        }
        Ok(())
    }

    fn read_atom<'tree>(
        &self,
        node: Node<'tree>,
        mut redirect_nodes: Vec<Node<'tree>>,
    ) -> Result<Atom<'tree>> {
        let mut command = node;
        while command.kind() == "redirected_statement" {
            let mut body = None;
            for part in literals::parts(command, self.budget)? {
                match part.field {
                    Some("body") if body.is_none() => body = Some(part.node),
                    Some("redirect") => redirect_nodes.push(part.node),
                    _ => return Err(UNSUPPORTED),
                }
            }
            command = body.ok_or(UNSUPPORTED)?;
        }
        if command.kind() != "command" {
            return Err(UNSUPPORTED);
        }

        let command_parts = literals::parts(command, self.budget)?;
        let evidence = crate::span(command, SourceId(0));
        let mut words = Vec::new();
        let mut assignments = Vec::new();
        let mut seen_name = false;
        for part in command_parts {
            match (part.field, part.node.kind()) {
                (Some("name"), "command_name") if !seen_name => {
                    let pieces = literals::parts(part.node, self.budget)?;
                    let [literal] = pieces.as_slice() else {
                        return Err(UNSUPPORTED);
                    };
                    words.push(self.word(literal.node)?);
                    seen_name = true;
                }
                (Some("argument"), _) if seen_name => words.push(self.argument_word(part.node)?),
                (Some("redirect"), _) => redirect_nodes.push(part.node),
                (None, "variable_assignment") if !seen_name => {
                    push_assignment(&mut assignments, self.assignment(part.node)?)?;
                }
                _ => return Err(UNSUPPORTED),
            }
        }
        if !seen_name {
            return Err(UNSUPPORTED);
        }
        redirect_nodes.sort_by_key(Node::start_byte);
        let mut redirects = Vec::new();
        let mut heredoc = None;
        for redirect in redirect_nodes {
            match redirect.kind() {
                "file_redirect" => {
                    redirects.push(redirections::file(redirect, self.source, self.budget)?);
                }
                "heredoc_redirect" if heredoc.is_none() => {
                    let mut parsed = redirections::heredoc(redirect, self.source, self.budget)?;
                    redirects.append(&mut parsed.redirects);
                    heredoc = Some(parsed);
                }
                _ => return Err(UNSUPPORTED),
            }
        }
        redirects.sort_by_key(redirections::FileRedirect::start_byte);
        if let Some(doc) = &heredoc {
            let preceding_end = redirects
                .iter()
                .filter(|redirect| redirect.end_byte() <= doc.node.start_byte())
                .map(redirections::FileRedirect::end_byte)
                .fold(command.end_byte(), usize::max);
            let gap = self
                .source
                .get(preceding_end..doc.node.start_byte())
                .ok_or(UNSUPPORTED)?;
            if !gap.trim().is_empty() {
                let trimmed = gap.trim();
                if trimmed != "-" {
                    return Err(UNSUPPORTED);
                }
                let start = preceding_end + gap.find('-').ok_or(UNSUPPORTED)?;
                words.push(Word {
                    value: "-".to_owned(),
                    evidence: literals::range(self.source, start, start + 1)?,
                    origin: WordOrigin::InheritedLiteral,
                });
            }
        }
        Ok(Atom {
            evidence,
            words,
            assignments,
            redirects,
            heredoc,
        })
    }

    fn word(&self, node: Node<'_>) -> Result<Word> {
        Ok(Word {
            value: literals::literal(node, self.source, self.budget)?,
            evidence: crate::span(node, SourceId(0)),
            origin: WordOrigin::InheritedLiteral,
        })
    }

    // Only direct command argument nodes use this extension. Names, assignment
    // values, redirect destinations and concatenation children retain the
    // inherited decoder; ownership-sensitive argv positions are checked below.
    fn argument_word(&self, node: Node<'_>) -> Result<Word> {
        self.budget.check()?;
        if node.kind() != "number" {
            return self.word(node);
        }
        let raw = crate::text(node, self.source);
        let digits = raw
            .strip_prefix('-')
            .or_else(|| raw.strip_prefix('+'))
            .unwrap_or(raw);
        if node.child_count() != 0
            || digits.is_empty()
            || !digits.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(UNSUPPORTED);
        }
        Ok(Word {
            value: raw.to_owned(),
            evidence: crate::span(node, SourceId(0)),
            origin: WordOrigin::DecimalArgument,
        })
    }

    fn assignment(&self, node: Node<'_>) -> Result<Assignment> {
        let parts = literals::parts(node, self.budget)?;
        let (name, rest) = parts.split_first().ok_or(UNSUPPORTED)?;
        if name.field != Some("name")
            || name.node.kind() != "variable_name"
            || !environment_name(crate::text(name.node, self.source))
        {
            return Err(UNSUPPORTED);
        }
        let (operator, value) = rest.split_first().ok_or(UNSUPPORTED)?;
        if operator.node.kind() != "=" || operator.node.is_named() || operator.field.is_some() {
            return Err(UNSUPPORTED);
        }
        let value = match value {
            [] if operator.node.end_byte() == node.end_byte() => String::new(),
            [value] if value.field == Some("value") => {
                literals::literal(value.node, self.source, self.budget)?
            }
            _ => return Err(UNSUPPORTED),
        };
        let name = crate::text(name.node, self.source).to_owned();
        let mut spelling = name.clone();
        spelling.push('=');
        spelling.push_str(&value);
        Ok(Assignment {
            name,
            spelling,
            evidence: crate::span(node, SourceId(0)),
        })
    }
}
