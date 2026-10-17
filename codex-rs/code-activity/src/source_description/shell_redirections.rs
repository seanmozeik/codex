//! Atom-owned redirections and byte-preserving quoted heredocs.

#[path = "shell_redirections/heredoc_body.rs"]
mod heredoc_body;

use heredoc_body::physical_body_start;

use super::DescriptionBudget;
use super::SourceDescriptionFallback;
use super::SourceReference;
use super::SourceReferenceBasis;
use super::SourceReferenceKind;
use super::literals;
use crate::SourceId;
use crate::Span;
use tree_sitter::Node;

type Result<T> = std::result::Result<T, SourceDescriptionFallback>;
const UNSUPPORTED: SourceDescriptionFallback = SourceDescriptionFallback::UnsupportedSource;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Descriptor {
    Stdin,
    Stdout,
    Stderr,
}

impl Descriptor {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "0" => Ok(Self::Stdin),
            "1" => Ok(Self::Stdout),
            "2" => Ok(Self::Stderr),
            _ => Err(UNSUPPORTED),
        }
    }
}

enum FileOperator {
    Input,
    Output,
    Append,
}

enum DuplicationOperator {
    Input,
    Output,
}

enum Destination {
    File {
        operator: FileOperator,
        path: String,
    },
    Descriptor {
        operator: DuplicationOperator,
    },
}

pub(super) struct FileRedirect {
    descriptor: Option<Descriptor>,
    destination: Destination,
    evidence: Span,
}

impl FileRedirect {
    pub(super) fn start_byte(&self) -> usize {
        self.evidence.start_byte
    }

    pub(super) fn end_byte(&self) -> usize {
        self.evidence.end_byte
    }

    pub(super) fn targets_stdin(&self) -> bool {
        let default = match &self.destination {
            Destination::File {
                operator: FileOperator::Input,
                ..
            }
            | Destination::Descriptor {
                operator: DuplicationOperator::Input,
            } => Descriptor::Stdin,
            _ => Descriptor::Stdout,
        };
        self.descriptor.unwrap_or(default) == Descriptor::Stdin
    }

    pub(super) fn into_reference(self) -> SourceReference {
        SourceReference {
            basis: SourceReferenceBasis::SourceSyntax,
            kind: SourceReferenceKind::ShellRedirection,
            evidence: self.evidence,
            literal: match self.destination {
                Destination::File { path, .. } => Some(path),
                Destination::Descriptor { .. } => None,
            },
        }
    }
}

pub(super) fn file(
    node: Node<'_>,
    source: &str,
    budget: DescriptionBudget,
) -> Result<FileRedirect> {
    if node.kind() != "file_redirect" {
        return Err(UNSUPPORTED);
    }
    let parts = literals::parts(node, budget)?;
    let (descriptor, parts) = match parts.first() {
        Some(part) if part.field == Some("descriptor") => {
            if part.node.kind() != "file_descriptor" {
                return Err(UNSUPPORTED);
            }
            (
                Some(Descriptor::parse(crate::text(part.node, source))?),
                parts.get(1..).ok_or(UNSUPPORTED)?,
            )
        }
        _ => (None, parts.as_slice()),
    };
    let [operator, destination] = parts else {
        return Err(UNSUPPORTED);
    };
    if operator.node.is_named()
        || operator.field.is_some()
        || destination.field != Some("destination")
    {
        return Err(UNSUPPORTED);
    }
    let destination = match operator.node.kind() {
        "<" | ">" | ">>" => Destination::File {
            operator: match operator.node.kind() {
                "<" => FileOperator::Input,
                ">" => FileOperator::Output,
                _ => FileOperator::Append,
            },
            path: literals::literal(destination.node, source, budget)?,
        },
        "<&" | ">&" => {
            if !matches!(destination.node.kind(), "word" | "number") {
                return Err(UNSUPPORTED);
            }
            Descriptor::parse(crate::text(destination.node, source))?;
            Destination::Descriptor {
                operator: if operator.node.kind() == "<&" {
                    DuplicationOperator::Input
                } else {
                    DuplicationOperator::Output
                },
            }
        }
        _ => return Err(UNSUPPORTED),
    };
    Ok(FileRedirect {
        descriptor,
        destination,
        evidence: crate::span(node, SourceId(0)),
    })
}

pub(super) struct HeaderSuffix<'tree> {
    pub(super) operator: Node<'tree>,
    pub(super) right: Node<'tree>,
}

pub(super) struct Heredoc<'tree> {
    pub(super) node: Node<'tree>,
    pub(super) evidence: Span,
    pub(super) redirects: Vec<FileRedirect>,
    pub(super) suffix: Option<HeaderSuffix<'tree>>,
}

pub(super) fn heredoc<'tree>(
    node: Node<'tree>,
    source: &str,
    budget: DescriptionBudget,
) -> Result<Heredoc<'tree>> {
    if node.kind() != "heredoc_redirect" {
        return Err(UNSUPPORTED);
    }
    let parts = literals::parts(node, budget)?;
    let mut index = 0;
    if let Some(part) = parts.first()
        && part.field == Some("descriptor")
    {
        if part.node.kind() != "file_descriptor"
            || Descriptor::parse(crate::text(part.node, source))? != Descriptor::Stdin
        {
            return Err(UNSUPPORTED);
        }
        index += 1;
    }
    let operator = parts.get(index).ok_or(UNSUPPORTED)?;
    if operator.node.kind() != "<<" || operator.node.is_named() || operator.field.is_some() {
        return Err(UNSUPPORTED);
    }
    index += 1;
    let start = parts.get(index).ok_or(UNSUPPORTED)?;
    if start.node.kind() != "heredoc_start" || start.field.is_some() {
        return Err(UNSUPPORTED);
    }
    let delimiter = crate::text(start.node, source);
    let quote = delimiter.as_bytes().first().copied().ok_or(UNSUPPORTED)?;
    if !matches!(quote, b'\'' | b'"') || delimiter.as_bytes().last() != Some(&quote) {
        return Err(UNSUPPORTED);
    }
    let marker = delimiter.get(1..delimiter.len() - 1).ok_or(UNSUPPORTED)?;
    if marker.is_empty()
        || !marker
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(UNSUPPORTED);
    }
    index += 1;

    let mut redirects = Vec::new();
    while let Some(part) = parts.get(index)
        && part.field == Some("redirect")
    {
        redirects.push(file(part.node, source, budget)?);
        index += 1;
    }
    let suffix = match parts.get(index) {
        Some(part) if part.node.kind() == "pipeline" && part.field.is_none() => {
            if !redirects.is_empty() {
                return Err(UNSUPPORTED);
            }
            let pipeline = literals::parts(part.node, budget)?;
            let [operator, right] = pipeline.as_slice() else {
                return Err(UNSUPPORTED);
            };
            if operator.node.kind() != "|"
                || operator.node.is_named()
                || operator.field.is_some()
                || right.field.is_some()
            {
                return Err(UNSUPPORTED);
            }
            index += 1;
            Some(HeaderSuffix {
                operator: operator.node,
                right: right.node,
            })
        }
        Some(part) if part.field == Some("operator") => {
            if !matches!(part.node.kind(), "&&" | "||") || part.node.is_named() {
                return Err(UNSUPPORTED);
            }
            let right = parts.get(index + 1).ok_or(UNSUPPORTED)?;
            if right.field != Some("right") {
                return Err(UNSUPPORTED);
            }
            index += 2;
            Some(HeaderSuffix {
                operator: part.node,
                right: right.node,
            })
        }
        _ => None,
    };
    // The pinned grammar hides its heredoc LF token from the public CST.
    // Preserve the last consumed header boundary before reading the direct body.
    let header_end = parts
        .get(index.checked_sub(1).ok_or(UNSUPPORTED)?)
        .ok_or(UNSUPPORTED)?
        .node
        .end_byte();
    let body = match parts.get(index) {
        Some(part) if part.node.kind() == "heredoc_body" && part.field.is_none() => {
            index += 1;
            Some(part.node)
        }
        _ => None,
    };
    let end = parts.get(index).ok_or(UNSUPPORTED)?;
    if end.node.kind() != "heredoc_end"
        || end.field.is_some()
        || crate::text(end.node, source) != marker
        || end.node.start_position().column != 0
        || parts.len() != index + 1
        || !matches!(
            source.as_bytes().get(end.node.end_byte()),
            None | Some(b'\n')
        )
    {
        return Err(UNSUPPORTED);
    }

    let first_body_byte = body.map_or(end.node.start_byte(), |body| body.start_byte());
    let physical_start = physical_body_start(node, source, header_end, first_body_byte, budget)?;
    let physical_end = end.node.start_byte();
    let physical = source
        .get(physical_start..physical_end)
        .ok_or(UNSUPPORTED)?;
    if let Some(body) = body {
        if body.start_byte() < physical_start || body.end_byte() > physical_end {
            return Err(UNSUPPORTED);
        }
        // The pinned raw-body scanner may skip only initial/trailing ASCII
        // trivia. Preserve those physical bytes rather than shortening ownership.
        let prefix = source
            .get(physical_start..body.start_byte())
            .ok_or(UNSUPPORTED)?;
        let suffix = source
            .get(body.end_byte()..physical_end)
            .ok_or(UNSUPPORTED)?;
        if !literals::ascii_trivia(prefix) || !literals::ascii_trivia(suffix) {
            return Err(UNSUPPORTED);
        }
        let mut cursor = body.walk();
        for child in body.named_children(&mut cursor) {
            budget.check()?;
            if child.kind() != "heredoc_content" {
                return Err(UNSUPPORTED);
            }
        }
    } else if !literals::ascii_trivia(physical) {
        return Err(UNSUPPORTED);
    }
    budget.check()?;
    Ok(Heredoc {
        node,
        evidence: literals::range(source, physical_start, physical_end)?,
        redirects,
        suffix,
    })
}
