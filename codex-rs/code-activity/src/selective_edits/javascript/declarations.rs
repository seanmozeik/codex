//! Finite lexical declarations, including direct canonical CommonJS destructuring.

use super::syntax;
use super::syntax::Mutability;
use crate::SourceDescriptionFallback as Fallback;
use tree_sitter::Node;

type Result<T> = std::result::Result<T, Fallback>;
const UNSUPPORTED: Fallback = Fallback::UnsupportedSource;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FilesystemMethod {
    Read,
    Write,
    Open,
    WriteDescriptor,
    Close,
}

impl FilesystemMethod {
    pub(super) fn from_name(name: &str) -> Option<Self> {
        match name {
            "readFileSync" => Some(Self::Read),
            "writeFileSync" => Some(Self::Write),
            "openSync" => Some(Self::Open),
            "writeSync" => Some(Self::WriteDescriptor),
            "closeSync" => Some(Self::Close),
            _ => None,
        }
    }
}

pub(super) enum BindingTarget<'source> {
    Name(&'source str),
    FilesystemFunctions(Vec<(FilesystemMethod, &'source str)>),
}

impl BindingTarget<'_> {
    pub(super) fn names(&self) -> Vec<&str> {
        match self {
            Self::Name(name) => vec![name],
            Self::FilesystemFunctions(bindings) => bindings.iter().map(|(_, name)| *name).collect(),
        }
    }
}

pub(super) struct Declaration<'tree, 'source> {
    pub(super) target: BindingTarget<'source>,
    pub(super) initializer: Node<'tree>,
    pub(super) mutable: Mutability,
}

pub(super) fn parse<'tree, 'source>(
    node: Node<'tree>,
    source: &'source str,
) -> Result<Declaration<'tree, 'source>> {
    syntax::punctuation(node, source, &["const", "let", ";"])?;
    let mutable = match crate::text(syntax::field(node, "kind")?, source) {
        "const" => Mutability::Const,
        "let" => Mutability::Let,
        _ => return Err(UNSUPPORTED),
    };
    let declarator = syntax::only(node)?;
    if declarator.kind() != "variable_declarator" {
        return Err(UNSUPPORTED);
    }
    syntax::punctuation(declarator, source, &["="])?;
    let (name, initializer) = syntax::pair(declarator, "name", "value")?;
    let target = if name.kind() == "identifier" {
        BindingTarget::Name(syntax::identifier_kind(name, source, "identifier")?)
    } else {
        if mutable != Mutability::Const
            || name.kind() != "object_pattern"
            || initializer.kind() != "call_expression"
        {
            return Err(UNSUPPORTED);
        }
        let (callee, _) = syntax::call(initializer, source)?;
        if syntax::identifier_kind(callee, source, "identifier")? != "require" {
            return Err(UNSUPPORTED);
        }
        BindingTarget::FilesystemFunctions(functions(name, source)?)
    };
    Ok(Declaration {
        target,
        initializer,
        mutable,
    })
}

fn functions<'source>(
    node: Node<'_>,
    source: &'source str,
) -> Result<Vec<(FilesystemMethod, &'source str)>> {
    syntax::punctuation(node, source, &["{", ",", "}"])?;
    let members = syntax::parts(node);
    if members.is_empty() || members.len() > 2 {
        return Err(UNSUPPORTED);
    }
    let mut bindings = Vec::new();
    for member in members {
        let (export, name) = match member.kind() {
            "shorthand_property_identifier_pattern" => {
                let name = syntax::identifier_kind(
                    member,
                    source,
                    "shorthand_property_identifier_pattern",
                )?;
                (name, name)
            }
            "pair_pattern" => {
                syntax::punctuation(member, source, &[":"])?;
                let (key, value) = syntax::pair(member, "key", "value")?;
                if key.kind() != "property_identifier" {
                    return Err(UNSUPPORTED);
                }
                (
                    crate::text(key, source),
                    syntax::identifier_kind(value, source, "identifier")?,
                )
            }
            _ => return Err(UNSUPPORTED),
        };
        let method = FilesystemMethod::from_name(export).ok_or(UNSUPPORTED)?;
        if !matches!(method, FilesystemMethod::Read | FilesystemMethod::Write) {
            return Err(UNSUPPORTED);
        }
        if bindings.iter().any(|(existing, _)| *existing == method) {
            return Err(UNSUPPORTED);
        }
        bindings.push((method, name));
    }
    Ok(bindings)
}
