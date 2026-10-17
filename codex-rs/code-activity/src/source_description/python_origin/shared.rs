//! Bounded Python syntax IR for the closed sequential source-description grammar.
//! No node or extraction result asserts runtime identity or execution effects.

use std::time::Instant;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Module {
    pub statements: Vec<Statement>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Statement {
    pub span: Span,
    pub kind: StatementKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StatementKind {
    /// Only the exact `from pathlib import Path [as alias]` import.
    ImportPath {
        alias: String,
    },
    Assign {
        name: String,
        value: Expr,
    },
    Expression(Expr),
    /// One synchronous context, a simple local target, no nested contexts.
    With {
        name: String,
        context: Expr,
        body: Vec<Statement>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    pub span: Span,
    pub kind: ExprKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExprKind {
    Name(String),
    Text(String),
    Integer(i64),
    List(Vec<Expr>),
    Attribute {
        value: Box<Expr>,
        name: String,
    },
    Call {
        function: Box<Expr>,
        args: Vec<Expr>,
        keywords: Vec<(String, Expr)>,
    },
    Binary {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
    },
    Unary {
        op: UnaryOp,
        value: Box<Expr>,
    },
    Subscript {
        value: Box<Expr>,
        index: Index,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BinaryOp {
    Add,
    Subtract,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UnaryOp {
    Plus,
    Minus,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Index {
    Item(Box<Expr>),
    Slice {
        lower: Option<Box<Expr>>,
        upper: Option<Box<Expr>>,
        step: Option<Box<Expr>>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Fallback {
    Syntax,
    Unsupported,
    Limit,
    Deadline,
}

/// One shared deadline, never reset for a decoded child source.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub deadline: Instant,
    pub max_source_bytes: usize,
    pub max_nodes: usize,
    pub max_depth: usize,
    pub max_retained_bytes: usize,
    pub max_references: usize,
}

impl Limits {
    pub fn check(self) -> Result<(), Fallback> {
        if Instant::now() >= self.deadline {
            Err(Fallback::Deadline)
        } else {
            Ok(())
        }
    }
}

/// Lowering must call this before allocating each node/string. Native parser
/// allocation occurs first and requires an external wall/RSS worker bound.
pub struct Budget {
    limits: Limits,
    nodes: usize,
    retained: usize,
}

impl Budget {
    pub fn new(source: &str, limits: Limits) -> Result<Self, Fallback> {
        limits.check()?;
        if source.len() > limits.max_source_bytes {
            return Err(Fallback::Limit);
        }
        Ok(Self {
            limits,
            nodes: 0,
            retained: 0,
        })
    }

    pub fn node(&mut self, depth: usize, bytes: usize) -> Result<(), Fallback> {
        self.limits.check()?;
        self.nodes = self.nodes.checked_add(1).ok_or(Fallback::Limit)?;
        self.retained = self.retained.checked_add(bytes).ok_or(Fallback::Limit)?;
        if depth > self.limits.max_depth
            || self.nodes > self.limits.max_nodes
            || self.retained > self.limits.max_retained_bytes
        {
            return Err(Fallback::Limit);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReferenceKind {
    Import,
    Path,
    Open,
    Read,
    Replace,
    Transform,
    Write,
    Output,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Reference {
    pub kind: ReferenceKind,
    pub span: Span,
    pub literal: Option<String>,
}
