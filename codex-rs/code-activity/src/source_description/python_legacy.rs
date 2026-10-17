//! Closed Python syntax origins; no callable identity or file effects inferred.

#[path = "python_expressions.rs"]
mod expressions;
#[path = "python_statements.rs"]
mod statements;

use super::DescriptionBudget;
use super::MAX_DEPTH;
use super::MAX_REFERENCES;
use super::MAX_RETAINED_BYTES;
use super::SourceDescriptionFallback;
use super::SourceReference;
use super::SourceReferenceBasis;
use super::SourceReferenceKind;
use crate::SourceId;
use std::collections::BTreeMap;
use tree_sitter::Node;
use tree_sitter::Tree;

const MAX_BINDINGS: usize = 256;
const MAX_LITERAL_BYTES: usize = 16_384;

#[derive(Clone)]
enum Origin {
    // An entry spelling or immutable local alias, not verified builtin identity.
    OpenName,
    Literal(String),
    PathCall(String),
    OpenCall { literal: String, mode: OpenMode },
    ContextOpen { literal: String, mode: OpenMode },
    ClosedContext,
    ReadCall(String),
    ReplacedText(Option<String>),
    ComposedText(Option<String>),
    DiscardedWrite,
    DiscardedOutput,
}

#[derive(Clone, Copy)]
enum OpenMode {
    Read,
    Write,
}

impl Origin {
    fn is_data(&self) -> bool {
        matches!(
            self,
            Self::Literal(_) | Self::ReadCall(_) | Self::ReplacedText(_) | Self::ComposedText(_)
        )
    }

    fn data_path(&self) -> Option<&str> {
        match self {
            Self::ReadCall(path) => Some(path),
            Self::ReplacedText(path) | Self::ComposedText(path) => path.as_deref(),
            _ => None,
        }
    }

    fn retained(&self) -> usize {
        match self {
            Self::Literal(value) | Self::PathCall(value) | Self::ReadCall(value) => value.len(),
            Self::OpenCall { literal, .. } | Self::ContextOpen { literal, .. } => literal.len(),
            Self::ReplacedText(value) | Self::ComposedText(value) => {
                value.as_ref().map_or(0, String::len)
            }
            Self::OpenName | Self::DiscardedWrite | Self::DiscardedOutput | Self::ClosedContext => {
                0
            }
        }
    }
}

struct State<'a> {
    source: &'a str,
    constructor: Option<String>,
    bindings: BTreeMap<String, Origin>,
    references: Vec<SourceReference>,
    retained: usize,
    budget: DescriptionBudget,
    failure: SourceDescriptionFallback,
}

pub(crate) fn describe(
    tree: &Tree,
    source: &str,
    budget: DescriptionBudget,
) -> Result<Vec<SourceReference>, SourceDescriptionFallback> {
    let mut state = State {
        source,
        constructor: None,
        bindings: BTreeMap::new(),
        references: vec![],
        retained: 0,
        budget,
        failure: SourceDescriptionFallback::UnsupportedSource,
    };
    let complete = state.run(tree.root_node());
    budget.check()?;
    complete.ok_or(state.failure)?;
    Ok(state.references)
}

impl State<'_> {
    fn is_open_name(&self, name: &str) -> bool {
        name == "open" || matches!(self.bindings.get(name), Some(Origin::OpenName))
    }

    fn run(&mut self, root: Node<'_>) -> Option<()> {
        for (index, statement) in parts(root).into_iter().enumerate() {
            if self.budget.expired() {
                return None;
            }
            if index == 0 && statement.kind() == "import_from_statement" {
                self.constructor = Some(import_name(statement, self.source)?);
                self.emit(
                    statement,
                    SourceReferenceKind::ImportFrom,
                    Some("pathlib.Path"),
                )?;
                continue;
            }
            self.statement(statement)?;
        }
        Some(())
    }

    fn retain(&mut self, bytes: usize) -> Option<()> {
        self.retained = self.retained.checked_add(bytes)?;
        if self.retained > MAX_RETAINED_BYTES {
            self.failure = SourceDescriptionFallback::SourceLimit;
            return None;
        }
        Some(())
    }

    fn emit(
        &mut self,
        node: Node<'_>,
        kind: SourceReferenceKind,
        literal: Option<&str>,
    ) -> Option<()> {
        if self.budget.expired() {
            return None;
        }
        if self.references.len() >= MAX_REFERENCES {
            self.failure = SourceDescriptionFallback::SourceLimit;
            return None;
        }
        self.retain(literal.map_or(0, str::len).checked_add(128)?)?;
        self.references.push(SourceReference {
            basis: SourceReferenceBasis::SourceSyntax,
            kind,
            evidence: crate::span(node, SourceId(0)),
            literal: literal.map(str::to_owned),
        });
        Some(())
    }
}

fn parts(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|child| child.kind() != "comment")
        .collect()
}

fn import_name(node: Node<'_>, source: &str) -> Option<String> {
    let tokens: Vec<_> = crate::text(node, source).split_whitespace().collect();
    let alias = match tokens.as_slice() {
        ["from", "pathlib", "import", "Path"] => "Path",
        ["from", "pathlib", "import", "Path", "as", alias] => alias,
        _ => return None,
    };
    let imported = parts(node);
    let [module, item] = imported.as_slice() else {
        return None;
    };
    if crate::text(*module, source) != "pathlib" || matches!(alias, "open" | "print") {
        return None;
    }
    if item.kind() == "aliased_import" {
        if item.child_by_field_name("alias")?.kind() != "identifier" {
            return None;
        }
    } else if crate::text(*item, source) != "Path" {
        return None;
    }
    identifier(alias).then(|| alias.to_owned())
}

fn identifier(name: &str) -> bool {
    name != "__debug__"
        && name.is_ascii()
        && name
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}
