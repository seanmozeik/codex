//! Closed JavaScript source origins; module and method names are syntax only.

#[path = "javascript_expressions.rs"]
mod expressions;

use super::DescriptionBudget;
use super::MAX_DEPTH;
use super::MAX_REFERENCES;
use super::MAX_RETAINED_BYTES;
use super::SourceDescriptionFallback;
use super::SourceReference;
use super::SourceReferenceBasis;
use super::SourceReferenceKind;
use crate::Language;
use crate::SourceId;
use std::collections::BTreeMap;
use tree_sitter::Node;

const MAX_BINDINGS: usize = 256;
const MAX_LITERAL_BYTES: usize = 16_384;

#[derive(Clone)]
enum Origin {
    Literal(String),
    Read(String),
    Replaced(Option<String>),
    Discarded,
}

impl Origin {
    fn retained(&self) -> usize {
        match self {
            Self::Literal(value) | Self::Read(value) => value.len(),
            Self::Replaced(value) => value.as_ref().map_or(0, String::len),
            Self::Discarded => 0,
        }
    }

    fn is_data(&self) -> bool {
        !matches!(self, Self::Discarded)
    }
}

struct Binding {
    origin: Origin,
    mutable: bool,
}

struct State<'a> {
    source: &'a str,
    module: Option<String>,
    bindings: BTreeMap<String, Binding>,
    references: Vec<SourceReference>,
    retained: usize,
    budget: DescriptionBudget,
    failure: SourceDescriptionFallback,
}

pub(super) fn extract(
    root: Node<'_>,
    source: &str,
    budget: DescriptionBudget,
) -> Result<Vec<SourceReference>, SourceDescriptionFallback> {
    if root.kind() != "program" || root.has_error() || source.contains('\0') {
        return Err(SourceDescriptionFallback::UnsupportedSource);
    }
    let mut state = State {
        source,
        module: None,
        bindings: BTreeMap::new(),
        references: vec![],
        retained: 0,
        budget,
        failure: SourceDescriptionFallback::UnsupportedSource,
    };
    let complete = state.run(root);
    budget.check()?;
    complete.ok_or(state.failure)?;
    Ok(state.references)
}

impl<'a> State<'a> {
    fn run(&mut self, root: Node<'_>) -> Option<()> {
        for (index, statement) in parts(root).into_iter().enumerate() {
            if self.budget.expired() {
                return None;
            }
            match statement.kind() {
                "import_statement" if index == 0 => self.import(statement)?,
                "lexical_declaration" => self.declaration(statement, index == 0)?,
                "expression_statement" => {
                    let nodes = parts(statement);
                    let [expression] = nodes.as_slice() else {
                        return None;
                    };
                    if expression.kind() == "assignment_expression" {
                        self.assign(*expression)?;
                    } else {
                        self.eval(*expression, 0)?;
                    }
                }
                _ => return None,
            }
        }
        Some(())
    }

    fn declaration(&mut self, node: Node<'_>, first: bool) -> Option<()> {
        let kind = crate::text(node.child_by_field_name("kind")?, self.source);
        let mutable = match kind {
            "let" => true,
            "const" => false,
            _ => return None,
        };
        let nodes = parts(node);
        let [declaration] = nodes.as_slice() else {
            return None;
        };
        if declaration.kind() != "variable_declarator" || parts(*declaration).len() != 2 {
            return None;
        }
        let mut cursor = declaration.walk();
        if declaration
            .children(&mut cursor)
            .any(|child| !child.is_named() && child.kind() != "=")
        {
            return None;
        }
        let name = declaration.child_by_field_name("name")?;
        let value = declaration.child_by_field_name("value")?;
        let name = self.name(name)?;
        if self.bindings.contains_key(name) || self.module.as_deref() == Some(name) {
            return None;
        }
        if first && !mutable && value.kind() == "call_expression" {
            let callee = value.child_by_field_name("function")?;
            if callee.kind() == "identifier" && crate::text(callee, self.source) == "require" {
                let arguments = call_arguments(value, self.source)?;
                let [argument] = arguments.as_slice() else {
                    return None;
                };
                let module = module_literal(*argument, self.source)?;
                self.module = Some(name.to_owned());
                return self.emit(node, SourceReferenceKind::ImportFrom, Some(&module));
            }
        }
        let origin = self.eval(value, 0)?;
        if !origin.is_data() {
            return None;
        }
        self.retain(name.len().checked_add(origin.retained())?)?;
        if self.bindings.len() >= MAX_BINDINGS {
            self.failure = SourceDescriptionFallback::SourceLimit;
            return None;
        }
        self.bindings
            .insert(name.to_owned(), Binding { origin, mutable });
        Some(())
    }

    fn assign(&mut self, node: Node<'_>) -> Option<()> {
        if parts(node).len() != 2 {
            return None;
        }
        let name = self.name(node.child_by_field_name("left")?)?;
        if !self.bindings.get(name)?.mutable {
            return None;
        }
        let origin = self.eval(node.child_by_field_name("right")?, 0)?;
        if !origin.is_data() {
            return None;
        }
        self.retain(origin.retained())?;
        self.bindings.get_mut(name)?.origin = origin;
        Some(())
    }

    fn import(&mut self, node: Node<'_>) -> Option<()> {
        let mut cursor = node.walk();
        if node
            .children(&mut cursor)
            .any(|child| !child.is_named() && !matches!(child.kind(), "import" | "from" | ";"))
        {
            return None;
        }
        let nodes = parts(node);
        let [clause, source] = nodes.as_slice() else {
            return None;
        };
        if clause.kind() != "import_clause" {
            return None;
        }
        let module = module_literal(*source, self.source)?;
        let bindings = parts(*clause);
        let [binding] = bindings.as_slice() else {
            return None;
        };
        let name = if binding.kind() == "namespace_import" {
            let nodes = parts(*binding);
            let [name] = nodes.as_slice() else {
                return None;
            };
            self.name(*name)?
        } else {
            self.name(*binding)?
        };
        self.module = Some(name.to_owned());
        self.emit(node, SourceReferenceKind::ImportFrom, Some(&module))
    }

    fn name(&self, node: Node<'_>) -> Option<&'a str> {
        let name = crate::text(node, self.source);
        (node.kind() == "identifier" && identifier(name)).then_some(name)
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

fn identifier(name: &str) -> bool {
    !matches!(
        name,
        "require"
            | "console"
            | "print"
            | "eval"
            | "arguments"
            | "__proto__"
            | "prototype"
            | "constructor"
            | "await"
            | "yield"
            | "let"
            | "enum"
            | "implements"
            | "interface"
            | "package"
            | "private"
            | "protected"
            | "public"
            | "static"
    ) && name
        .bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || matches!(byte, b'_' | b'$'))
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$'))
}

fn literal(node: Node<'_>, source: &str) -> Option<String> {
    let raw = crate::text(node, source);
    if node.kind() != "string"
        || raw.len() > MAX_LITERAL_BYTES + 8
        || raw.contains('\r')
        || !matches!(raw.as_bytes().first(), Some(b'\'' | b'"'))
    {
        return None;
    }
    let value = crate::value::string_literal(raw, Language::TypeScript)?;
    (value.len() <= MAX_LITERAL_BYTES).then_some(value)
}

fn module_literal(node: Node<'_>, source: &str) -> Option<String> {
    let value = literal(node, source)?;
    matches!(value.as_str(), "fs" | "node:fs").then_some(value)
}

fn call_arguments<'tree>(node: Node<'tree>, source: &str) -> Option<Vec<Node<'tree>>> {
    if parts(node).len() != 2 {
        return None;
    }
    let function = node.child_by_field_name("function")?;
    let arguments = node.child_by_field_name("arguments")?;
    if arguments.kind() != "arguments"
        || !source
            .get(function.end_byte()..arguments.start_byte())?
            .trim()
            .is_empty()
    {
        return None;
    }
    Some(parts(arguments))
}
