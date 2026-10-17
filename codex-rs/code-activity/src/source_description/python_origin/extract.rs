//! One atomic source-origin extractor shared by both parser frontends.

#[path = "extract_calls.rs"]
mod calls;
#[path = "extract_expr.rs"]
mod expressions;

use crate::source_description::python::origin::shared::Expr;
use crate::source_description::python::origin::shared::Fallback;
use crate::source_description::python::origin::shared::Limits;
use crate::source_description::python::origin::shared::Module;
use crate::source_description::python::origin::shared::Reference;
use crate::source_description::python::origin::shared::ReferenceKind;
use crate::source_description::python::origin::shared::Span;
use crate::source_description::python::origin::shared::Statement;
use crate::source_description::python::origin::shared::StatementKind;
use std::collections::BTreeMap;

#[derive(Clone)]
enum Text {
    Literal(String),
    Derived(Option<String>),
}

impl Text {
    fn path(&self) -> Option<&str> {
        match self {
            Self::Literal(_) => None,
            Self::Derived(path) => path.as_deref(),
        }
    }

    fn bytes(&self) -> usize {
        match self {
            Self::Literal(text) => text.len(),
            Self::Derived(path) => path.as_ref().map_or(0, String::len),
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Mode {
    Read,
    Write,
}

#[derive(Clone)]
enum Value {
    OpenName,
    Path(String),
    Handle {
        path: String,
        mode: Mode,
        scoped: bool,
    },
    Text(Text),
    TextList(Option<String>),
    Integer(Option<i64>),
    Discard,
    Closed,
}

impl Value {
    fn data(&self) -> bool {
        matches!(self, Self::Text(_) | Self::TextList(_) | Self::Integer(_))
    }

    fn bytes(&self) -> usize {
        match self {
            Self::Path(path) | Self::Handle { path, .. } => path.len(),
            Self::Text(text) => text.bytes(),
            Self::TextList(path) => path.as_ref().map_or(0, String::len),
            _ => 0,
        }
    }

    fn same_binding_family(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (Self::Path(_), Self::Path(_))
                | (Self::Text(_), Self::Text(_))
                | (Self::TextList(_), Self::TextList(_))
                | (Self::Integer(_), Self::Integer(_))
        )
    }
}

struct State {
    limits: Limits,
    retained: usize,
    visits: usize,
    constructor: Option<String>,
    bindings: BTreeMap<String, Value>,
    references: Vec<Reference>,
    context_depth: usize,
}

/// Returns references only after every statement and child expression succeeds.
/// These references describe syntax, not effects or verified callable identities.
pub fn describe(module: &Module, limits: Limits) -> Result<Vec<Reference>, Fallback> {
    let mut state = State {
        limits,
        retained: 0,
        visits: 0,
        constructor: None,
        bindings: BTreeMap::new(),
        references: Vec::new(),
        context_depth: 0,
    };
    for (index, statement) in module.statements.iter().enumerate() {
        if let StatementKind::ImportPath { alias } = &statement.kind {
            if index != 0
                || !local_name(alias)
                || matches!(alias.as_str(), "open" | "print" | "len")
            {
                return Err(Fallback::Unsupported);
            }
            state.charge(alias.len())?;
            state.constructor = Some(alias.clone());
            state.emit(statement.span, ReferenceKind::Import, Some("pathlib.Path"))?;
        } else {
            state.statement(statement)?;
        }
    }
    limits.check()?;
    Ok(state.references)
}

impl State {
    fn charge(&mut self, bytes: usize) -> Result<(), Fallback> {
        self.limits.check()?;
        self.retained = self.retained.checked_add(bytes).ok_or(Fallback::Limit)?;
        if self.retained > self.limits.max_retained_bytes {
            return Err(Fallback::Limit);
        }
        Ok(())
    }

    fn visit(&mut self, depth: usize) -> Result<(), Fallback> {
        self.limits.check()?;
        self.visits = self.visits.checked_add(1).ok_or(Fallback::Limit)?;
        if self.visits > self.limits.max_nodes || depth > self.limits.max_depth {
            return Err(Fallback::Limit);
        }
        Ok(())
    }

    fn emit(
        &mut self,
        span: Span,
        kind: ReferenceKind,
        literal: Option<&str>,
    ) -> Result<(), Fallback> {
        if self.references.len() >= self.limits.max_references {
            return Err(Fallback::Limit);
        }
        self.charge(128 + literal.map_or(0, str::len))?;
        self.references.push(Reference {
            span,
            kind,
            literal: literal.map(str::to_owned),
        });
        Ok(())
    }

    fn name_available(&self, name: &str) -> bool {
        local_name(name)
            && !matches!(name, "open" | "print" | "len")
            && self.constructor.as_deref() != Some(name)
    }

    fn bind(&mut self, name: &str, value: Value) -> Result<(), Fallback> {
        if self.bindings.len() >= 256 && !self.bindings.contains_key(name) {
            return Err(Fallback::Limit);
        }
        self.charge(128 + name.len() + value.bytes())?;
        self.bindings.insert(name.to_owned(), value);
        Ok(())
    }

    fn statement(&mut self, statement: &Statement) -> Result<(), Fallback> {
        self.visit(0)?;
        match &statement.kind {
            StatementKind::ImportPath { .. } => Err(Fallback::Unsupported),
            StatementKind::Expression(expression) => {
                let value = self.eval(expression, 0)?;
                if matches!(value, Value::OpenName | Value::Closed) {
                    return Err(Fallback::Unsupported);
                }
                Ok(())
            }
            StatementKind::Assign { name, value } => {
                if !self.name_available(name) {
                    return Err(Fallback::Unsupported);
                }
                let origin = self.eval(value, 0)?;
                if matches!(
                    origin,
                    Value::Discard | Value::Closed | Value::Handle { scoped: true, .. }
                ) || self
                    .bindings
                    .get(name)
                    .is_some_and(|old| !old.same_binding_family(&origin))
                {
                    return Err(Fallback::Unsupported);
                }
                self.bind(name, origin)
            }
            StatementKind::With {
                name,
                context,
                body,
            } => self.context(name, context, body),
        }
    }

    fn context(&mut self, name: &str, context: &Expr, body: &[Statement]) -> Result<(), Fallback> {
        if self.context_depth != 0 || !self.name_available(name) || self.bindings.contains_key(name)
        {
            return Err(Fallback::Unsupported);
        }
        let Value::Handle {
            path,
            mode,
            scoped: false,
        } = self.eval(context, 0)?
        else {
            return Err(Fallback::Unsupported);
        };
        // Only a direct open call can introduce the context; an earlier handle
        // could escape through an alias and outlive the tracked scope.
        if !matches!(
            context.kind,
            crate::source_description::python::origin::shared::ExprKind::Call { .. }
        ) {
            return Err(Fallback::Unsupported);
        }
        self.bind(
            name,
            Value::Handle {
                path,
                mode,
                scoped: true,
            },
        )?;
        self.context_depth = 1;
        for statement in body {
            self.statement(statement)?;
        }
        self.context_depth = 0;
        self.bindings.insert(name.to_owned(), Value::Closed);
        Ok(())
    }
}

fn local_name(name: &str) -> bool {
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
