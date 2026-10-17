//! Immutable captured values and the finite capability registry.

use super::MAX_RECIPE_DEPTH;
use super::MAX_RECORDS;
use super::MAX_RETAINED_BYTES;
use super::contexts::HandleId;
use super::dependency_weight;
use super::strings;
use super::summaries::PredicateValue;
use super::target_weight;
use crate::Span;
use crate::Target;
use crate::selective_edits::python::PythonDependency;
use crate::selective_edits::python::PythonWriteNewline;
use crate::selective_edits::python::TextEncoding;
use crate::selective_edits::python::TextRecipe;

#[derive(Clone)]
pub(super) struct Value {
    pub(super) kind: Kind,
    pub(super) dependencies: Vec<PythonDependency>,
    pub(super) weight: usize,
}

impl Value {
    pub(super) fn is_join(&self) -> bool {
        matches!(
            &self.kind,
            Kind::Method {
                method: Method::Join,
                ..
            }
        )
    }

    pub(super) fn new(kind: Kind, dependencies: Vec<PythonDependency>) -> Self {
        let inner = match &kind {
            Kind::Text(value) => value.weight,
            Kind::Predicate(value) => value.weight,
            Kind::Index(value) => value.weight,
            Kind::ListText(value) => value.weight,
            Kind::Path(target) | Kind::Handle { target, .. } => target_weight(target),
            Kind::Method { receiver, .. } => receiver.weight,
            _ => 0,
        };
        let weight = 128 + inner + dependencies.iter().map(dependency_weight).sum::<usize>();
        Self {
            kind,
            dependencies,
            weight,
        }
    }

    pub(super) fn literal(&self) -> Option<&str> {
        match &self.kind {
            Kind::Text(TextValue {
                recipe: TextRecipe::Literal { value, .. },
                ..
            }) => Some(value),
            _ => None,
        }
    }

    /// Only source literals and immutable text concatenation can establish a
    /// constant here. Reads and other recipes never become analyzer-host paths.
    pub(super) fn constant_text(&self) -> Option<String> {
        fn collect<'a>(
            recipe: &'a TextRecipe,
            depth: usize,
            bytes: &mut usize,
            pieces: &mut Vec<&'a str>,
        ) -> Option<()> {
            if depth > MAX_RECIPE_DEPTH || pieces.len() >= MAX_RECORDS {
                return None;
            }
            match recipe {
                TextRecipe::Literal { value, .. } => {
                    *bytes = bytes.checked_add(value.len())?;
                    if *bytes > MAX_RETAINED_BYTES {
                        return None;
                    }
                    pieces.push(value);
                }
                TextRecipe::Concat { left, right, .. } => {
                    collect(left, depth + 1, bytes, pieces)?;
                    collect(right, depth + 1, bytes, pieces)?;
                }
                _ => return None,
            }
            Some(())
        }
        let Kind::Text(text) = &self.kind else {
            return None;
        };
        let mut bytes = 0;
        let mut pieces = Vec::new();
        collect(&text.recipe, 1, &mut bytes, &mut pieces)?;
        let mut result = String::with_capacity(bytes);
        for piece in pieces {
            result.push_str(piece);
        }
        Some(result)
    }

    pub(super) fn hashable(&self) -> bool {
        match &self.kind {
            Kind::Text(_)
            | Kind::Integer(_)
            | Kind::Index(_)
            | Kind::Predicate(_)
            | Kind::Primitive => true,
            Kind::Container { hashable } => *hashable,
            _ => false,
        }
    }

    pub(super) fn has_handle(&self) -> bool {
        match &self.kind {
            Kind::Handle { .. } | Kind::ContextHandle(_) => true,
            Kind::Method { receiver, .. } => receiver.has_handle(),
            _ => false,
        }
    }
}

#[derive(Clone)]
pub(super) struct TextValue {
    pub(super) recipe: TextRecipe,
    pub(super) depth: usize,
    pub(super) weight: usize,
}

#[derive(Clone)]
pub(super) enum Kind {
    Text(TextValue),
    Predicate(PredicateValue),
    Integer(Option<i64>),
    Index(strings::IndexValue),
    ListText(strings::SplitValue),
    Primitive,
    Container {
        hashable: bool,
    },
    Path(Target),
    PathFunction,
    PathlibModule,
    OpenFunction,
    DeferredFunction,
    ContextHandle(HandleId),
    Handle {
        target: Target,
        mode: Mode,
        encoding: TextEncoding,
        newline: Option<PythonWriteNewline>,
    },
    Method {
        receiver: Box<Value>,
        method: Method,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    Read,
    Write,
    Update,
}

#[derive(Clone, Copy)]
pub(super) enum Method {
    OpenRead,
    OpenWrite,
    ContextRead,
    ContextWrite,
    ContextClose,
    ContextSeek,
    PathRead,
    PathWrite,
    Replace,
    Join,
    Index,
    Split,
    Rstrip,
    Count,
    Find,
    StartsWith,
    EndsWith,
}

#[derive(Clone)]
pub(super) struct Binding {
    pub(super) value: Value,
    pub(super) version: usize,
    pub(super) evidence: Span,
}
