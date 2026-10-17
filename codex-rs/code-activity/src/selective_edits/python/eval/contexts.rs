//! Owned handles with legacy close writes or explicit-policy logical endpoints.

mod logical;
mod suffix;

use super::Kind;
use super::MAX_RECORDS;
use super::Method;
use super::Mode;
use super::Result;
use super::State;
use super::Value;
use super::calls::Argument;
use super::target_weight;
use crate::SourceDescriptionFallback as Fallback;
use crate::Span;
use crate::Target;
use crate::selective_edits::PythonStreamMode;
use crate::selective_edits::PythonStreamPosition;
use crate::selective_edits::PythonWriteEndpoint;
use crate::selective_edits::PythonWriteNewline;
use crate::selective_edits::python::PythonBarrierReason;
use crate::selective_edits::python::PythonDependency;
use crate::selective_edits::python::PythonOperation;
use crate::selective_edits::python::TextEncoding;
use crate::selective_edits::python::TextRecipe;
use crate::selective_edits::python::syntax::contexts::shape;
use crate::selective_edits::python::syntax::field;
use crate::selective_edits::python::syntax::parts;
use crate::selective_edits::python::syntax::unparenthesized;
use tree_sitter::Node;

#[derive(Clone, Copy)]
pub(super) struct HandleId(usize);

pub(super) struct ContextHandle {
    target: Target,
    state: HandleState,
    newline: Option<PythonWriteNewline>,
    opened_at: Span,
    base_epoch: usize,
}

enum HandleState {
    Readable,
    Writable,
    Read,
    Pending(Box<PendingWrite>),
    Closed,
    Logical {
        mode: PythonStreamMode,
        position: PythonStreamPosition,
    },
    LogicalRead {
        read_id: usize,
        read_evidence: Span,
    },
    LogicalWritten,
}

struct PendingWrite {
    content: TextRecipe,
    dependencies: Vec<PythonDependency>,
}

impl State<'_> {
    pub(super) fn context(&mut self, node: Node<'_>) -> Result<()> {
        let shape = shape(node, self.source)?;
        let manager = unparenthesized(shape.manager)?;
        if self.has_open_context() || manager.kind() != "call" {
            return self.halt(manager, PythonBarrierReason::UnsupportedOperation);
        }
        let function = unparenthesized(field(manager, "function")?)?;
        if function.kind() != "identifier"
            || crate::text(function, self.source) != "open"
            || self.bindings.contains_key("open")
        {
            return self.halt(manager, PythonBarrierReason::UnmodeledCall);
        }
        let value = self.expression(manager)?;
        let Kind::Handle {
            target,
            mode,
            encoding: TextEncoding::Utf8,
            newline,
        } = value.kind
        else {
            return self.halt(manager, PythonBarrierReason::UnsupportedOperation);
        };
        if self.context_handles.len() >= MAX_RECORDS {
            return Err(Fallback::SourceLimit.into());
        }
        self.guard.charge(128)?;
        let id = HandleId(self.context_handles.len());
        self.context_handles.push(ContextHandle {
            target,
            newline,
            opened_at: self.evidence(manager),
            base_epoch: self.filesystem_epoch,
            state: match mode {
                Mode::Read => HandleState::Readable,
                Mode::Write
                    if newline.is_none()
                        || self.python_write_endpoint
                            == PythonWriteEndpoint::ContextCloseWhenAvailable =>
                {
                    HandleState::Writable
                }
                Mode::Write => HandleState::Logical {
                    mode: PythonStreamMode::TruncateWrite,
                    position: PythonStreamPosition::InitialZero,
                },
                Mode::Update => HandleState::Logical {
                    mode: PythonStreamMode::UpdateExisting,
                    position: PythonStreamPosition::InitialZero,
                },
            },
        });
        self.bind(
            crate::text(shape.owner, self.source),
            Value::new(Kind::ContextHandle(id), value.dependencies),
            shape.binding,
        )?;
        let mut statements = parts(shape.body);
        while let Some(statement) = statements.next() {
            if !matches!(statement.kind(), "pass_statement" | "expression_statement") {
                return self.halt(statement, PythonBarrierReason::UnsupportedOperation);
            }
            if let Err(stop) = self.statement(statement) {
                if matches!(stop, super::Stop::Barrier) {
                    self.summarize_suffix_touch(id, statement, statements.next())?;
                }
                return Err(stop);
            }
        }
        // The successful enclosing statement includes __exit__/close. Pending
        // bytes are never exposed as an immediately applied write at f.write.
        self.close_context(node, id)
    }

    pub(super) fn has_open_context(&self) -> bool {
        self.context_handles
            .iter()
            .any(|handle| !matches!(&handle.state, HandleState::Closed))
    }

    pub(super) fn guard_context_io(&mut self, node: Node<'_>, function: &Kind) -> Result<()> {
        if self.logical_endpoint
            && matches!(
                function,
                Kind::OpenFunction
                    | Kind::Method {
                        method: Method::OpenRead
                            | Method::OpenWrite
                            | Method::PathRead
                            | Method::PathWrite
                            | Method::ContextRead
                            | Method::ContextWrite
                            | Method::ContextSeek,
                        ..
                    }
            )
        {
            return self.halt(node, PythonBarrierReason::UnsupportedOperation);
        }
        if self.has_open_context()
            && matches!(
                function,
                Kind::OpenFunction
                    | Kind::Method {
                        method: Method::OpenRead
                            | Method::OpenWrite
                            | Method::PathRead
                            | Method::PathWrite,
                        ..
                    }
            )
        {
            // A separate file operation may alias the owned descriptor. Its
            // cursor and buffered bytes cannot be reconstructed from paths.
            return self.halt(node, PythonBarrierReason::UnsupportedOperation);
        }
        Ok(())
    }

    pub(super) fn is_context_owner(&self, name: &str) -> bool {
        self.bindings.get(name).is_some_and(|binding| {
            let Kind::ContextHandle(id) = &binding.value.kind else {
                return false;
            };
            self.context_handles
                .get(id.0)
                .is_some_and(|handle| !matches!(&handle.state, HandleState::Closed))
        })
    }

    pub(super) fn context_call(
        &mut self,
        node: Node<'_>,
        receiver: Value,
        method: Method,
        arguments: Vec<Argument>,
        dependencies: Vec<PythonDependency>,
    ) -> Result<Value> {
        let Kind::ContextHandle(id) = receiver.kind else {
            return Err(Fallback::UnsupportedSource.into());
        };
        match method {
            Method::ContextRead => self.read_context(node, id, arguments, dependencies),
            Method::ContextSeek => self.seek_context(node, id, arguments),
            Method::ContextWrite => {
                self.write_context(node, id, arguments, dependencies)?;
                Ok(Value::new(Kind::Integer(None), Vec::new()))
            }
            Method::ContextClose => {
                if !arguments.is_empty() {
                    return self.halt(node, PythonBarrierReason::UnsupportedOperation);
                }
                self.close_context(node, id)?;
                Ok(Value::new(Kind::Primitive, Vec::new()))
            }
            _ => Err(Fallback::UnsupportedSource.into()),
        }
    }

    fn write_context(
        &mut self,
        node: Node<'_>,
        id: HandleId,
        arguments: Vec<Argument>,
        dependencies: Vec<PythonDependency>,
    ) -> Result<()> {
        if arguments.len() != 1 || arguments[0].name.is_some() {
            return self.halt(node, PythonBarrierReason::UnsupportedOperation);
        }
        let argument = arguments
            .into_iter()
            .next()
            .ok_or(Fallback::UnsupportedSource)?;
        let Kind::Text(content) = argument.value.kind else {
            return self.halt(node, PythonBarrierReason::UnsupportedOperation);
        };
        if matches!(
            self.context_handles.get(id.0).map(|handle| &handle.state),
            Some(HandleState::Logical { .. })
        ) {
            return self.logical_write(node, id, content.recipe, dependencies);
        }
        let handle = self
            .context_handles
            .get_mut(id.0)
            .ok_or(Fallback::UnsupportedSource)?;
        if !matches!(&handle.state, HandleState::Writable) {
            return self.halt(node, PythonBarrierReason::UnsupportedOperation);
        }
        self.guard.charge(128)?;
        handle.state = HandleState::Pending(Box::new(PendingWrite {
            content: content.recipe,
            dependencies,
        }));
        self.condition(node)
    }

    fn close_context(&mut self, node: Node<'_>, id: HandleId) -> Result<()> {
        let handle = self
            .context_handles
            .get_mut(id.0)
            .ok_or(Fallback::UnsupportedSource)?;
        let state = std::mem::replace(&mut handle.state, HandleState::Closed);
        if let HandleState::Pending(write) = state {
            self.guard.charge(target_weight(&handle.target))?;
            let target = handle.target.clone();
            let newline = handle.newline;
            self.emit(
                node,
                target,
                PythonOperation::WriteText {
                    content: write.content,
                    encoding: TextEncoding::Utf8,
                    newline,
                },
                write.dependencies,
            )?;
        }
        self.condition(node)
    }
}
