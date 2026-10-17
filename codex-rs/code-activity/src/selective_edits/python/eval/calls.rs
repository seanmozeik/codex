//! Exact standard API signatures and their Python evaluation-order effects.

mod open;
mod options;
mod replace;

use options::method_options;

use super::Kind;
use super::MAX_RECORDS;
use super::Method;
use super::Mode;
use super::Result;
use super::State;
use super::TextValue;
use super::Value;
use super::target_weight;
use crate::SourceDescriptionFallback as Fallback;
use crate::Target;
use crate::selective_edits::python::PythonBarrierReason;
use crate::selective_edits::python::PythonDependency;
use crate::selective_edits::python::PythonOperation;
use crate::selective_edits::python::TextEncoding;
use crate::selective_edits::python::TextRecipe;
use crate::selective_edits::python::syntax::field;
use crate::selective_edits::python::syntax::parts;
use crate::selective_edits::python::syntax::unparenthesized;
use tree_sitter::Node;

pub(super) struct Argument {
    pub(super) name: Option<String>,
    pub(super) value: Value,
    pub(super) evidence: crate::Span,
    direct_literal: bool,
    direct_none: bool,
}

impl State<'_> {
    pub(super) fn call(&mut self, node: Node<'_>) -> Result<Value> {
        let function = self.expression(field(node, "function")?)?;
        if function.is_join() {
            return self.join(node, function);
        }
        if !matches!(
            &function.kind,
            Kind::OpenFunction | Kind::PathFunction | Kind::Method { .. }
        ) {
            // Opaque resolution/calling semantics cannot establish that nested
            // arguments are reached, so do not harvest writes from them.
            return self.halt(node, PythonBarrierReason::UnmodeledCall);
        }
        self.guard_context_io(node, &function.kind)?;
        if let Kind::Method {
            receiver,
            method: Method::OpenWrite,
        } = &function.kind
            && matches!(
                &receiver.kind,
                Kind::Handle {
                    newline: Some(_),
                    ..
                }
            )
        {
            // Explicit-policy buffered writes require the owned context lane.
            // Function resolution may already have truncated the destination;
            // do not harvest payload effects or infer physical write visibility.
            return self.halt(node, PythonBarrierReason::UnsupportedOperation);
        }
        let arguments = self.arguments(field(node, "arguments")?)?;
        let mut dependencies = function.dependencies;
        for argument in &arguments {
            let values = self.copy_dependencies(&argument.value.dependencies)?;
            self.merge(&mut dependencies, values)?;
        }
        match function.kind {
            Kind::OpenFunction => self.open(node, arguments, dependencies),
            Kind::PathFunction => self.path(node, arguments, dependencies),
            Kind::Method { receiver, method } => match method {
                Method::ContextRead
                | Method::ContextWrite
                | Method::ContextClose
                | Method::ContextSeek => {
                    self.context_call(node, *receiver, method, arguments, dependencies)
                }
                Method::OpenRead | Method::PathRead => {
                    self.read(node, *receiver, method, arguments, dependencies)
                }
                Method::OpenWrite | Method::PathWrite => {
                    self.write(node, *receiver, method, arguments, dependencies)
                }
                Method::Replace => self.replace(node, *receiver, arguments, dependencies),
                Method::Join => Err(Fallback::UnsupportedSource.into()),
                Method::Index | Method::Split | Method::Rstrip => {
                    self.string_method(node, *receiver, method, arguments, dependencies)
                }
                Method::Count | Method::Find | Method::StartsWith | Method::EndsWith => {
                    self.summarized_method(node, *receiver, method, arguments, dependencies)
                }
            },
            _ => Err(Fallback::UnsupportedSource.into()),
        }
    }

    fn arguments(&mut self, node: Node<'_>) -> Result<Vec<Argument>> {
        let mut arguments = Vec::new();
        for argument in parts(node) {
            if arguments.len() >= MAX_RECORDS {
                return Err(Fallback::SourceLimit.into());
            }
            let (name, value_node) = if argument.kind() == "keyword_argument" {
                let name = crate::text(field(argument, "name")?, self.source);
                self.guard.charge(name.len())?;
                (Some(name.to_owned()), field(argument, "value")?)
            } else {
                (None, argument)
            };
            let value = self.expression(value_node)?;
            if value.has_handle() {
                return self.halt(value_node, PythonBarrierReason::EscapingHandle);
            }
            self.guard.charge(64)?;
            arguments.push(Argument {
                evidence: self.evidence(value_node),
                name,
                value,
                direct_literal: unparenthesized(value_node)?.kind() == "string",
                direct_none: unparenthesized(value_node)?.kind() == "none",
            });
        }
        Ok(arguments)
    }

    fn path(
        &mut self,
        node: Node<'_>,
        arguments: Vec<Argument>,
        mut dependencies: Vec<PythonDependency>,
    ) -> Result<Value> {
        if arguments.len() != 1 || arguments[0].name.is_some() {
            return self.halt(node, PythonBarrierReason::UnsupportedOperation);
        }
        let target = self.target(node, &arguments[0].value, &mut dependencies)?;
        Ok(Value::new(Kind::Path(target), dependencies))
    }

    fn target(
        &mut self,
        node: Node<'_>,
        value: &Value,
        dependencies: &mut Vec<PythonDependency>,
    ) -> Result<Target> {
        let Some(path) = value
            .constant_text()
            .filter(|path| !path.is_empty() && !path.contains('\0'))
        else {
            return self.halt(node, PythonBarrierReason::UnsupportedOperation);
        };
        self.guard
            .charge(path.len() + self.cwd.map_or(0, str::len) * 2 + 128)?;
        self.dependency(
            dependencies,
            PythonDependency::EntryWorkingDirectory {
                cwd: self.cwd.map(str::to_owned),
            },
        )?;
        Ok(Target::Literal {
            path,
            cwd: self.cwd.map(str::to_owned),
        })
    }

    pub(super) fn read(
        &mut self,
        node: Node<'_>,
        receiver: Value,
        method: Method,
        arguments: Vec<Argument>,
        mut dependencies: Vec<PythonDependency>,
    ) -> Result<Value> {
        let (target, encoding, newline) = match (method, receiver.kind) {
            (
                Method::OpenRead,
                Kind::Handle {
                    target,
                    mode: Mode::Read,
                    encoding,
                    newline,
                },
            ) => {
                if !arguments.is_empty() {
                    return self.halt(node, PythonBarrierReason::UnsupportedOperation);
                }
                (target, encoding, newline)
            }
            (Method::PathRead, Kind::Path(target)) => {
                let Some((encoding, newline)) = method_options(&arguments, 0) else {
                    return self.halt(node, PythonBarrierReason::UnsupportedOperation);
                };
                self.pathlib_newline(newline, &mut dependencies)?;
                (target, encoding, newline)
            }
            _ => return Err(Fallback::UnsupportedSource.into()),
        };
        if self.next_read >= MAX_RECORDS {
            return Err(Fallback::SourceLimit.into());
        }
        let read_id = self.next_read;
        self.next_read += 1;
        self.dependency(
            &mut dependencies,
            PythonDependency::ReadValue {
                read_id,
                evidence: self.evidence(node),
            },
        )?;
        self.dependency(
            &mut dependencies,
            PythonDependency::FilesystemEpoch {
                epoch: self.filesystem_epoch,
            },
        )?;
        self.guard.charge(128)?;
        let weight = target_weight(&target) + 128;
        let recipe = TextRecipe::Read {
            read_id,
            target,
            evidence: self.evidence(node),
            filesystem_epoch: self.filesystem_epoch,
            encoding,
            newline: newline.map(options::read_newline),
        };
        self.condition(node)?;
        Ok(Value::new(
            Kind::Text(TextValue {
                recipe,
                depth: 1,
                weight,
            }),
            dependencies,
        ))
    }

    fn write(
        &mut self,
        node: Node<'_>,
        receiver: Value,
        method: Method,
        arguments: Vec<Argument>,
        mut dependencies: Vec<PythonDependency>,
    ) -> Result<Value> {
        let (target, encoding, newline) = match (method, receiver.kind) {
            (
                Method::OpenWrite,
                Kind::Handle {
                    target,
                    mode: Mode::Write,
                    encoding,
                    newline,
                },
            ) => {
                if arguments.len() != 1 || arguments[0].name.is_some() {
                    return self.halt(node, PythonBarrierReason::UnsupportedOperation);
                }
                (target, encoding, newline)
            }
            (Method::PathWrite, Kind::Path(target)) => {
                let Some((encoding, newline)) = method_options(&arguments, 1) else {
                    return self.halt(node, PythonBarrierReason::UnsupportedOperation);
                };
                self.pathlib_newline(newline, &mut dependencies)?;
                (target, encoding, newline)
            }
            _ => return Err(Fallback::UnsupportedSource.into()),
        };
        let mut arguments = arguments.into_iter();
        let content = arguments.next().ok_or(Fallback::UnsupportedSource)?.value;
        let Kind::Text(content) = content.kind else {
            return self.halt(node, PythonBarrierReason::UnsupportedOperation);
        };
        self.emit(
            node,
            target,
            PythonOperation::WriteText {
                content: content.recipe,
                encoding,
                newline,
            },
            dependencies,
        )?;
        self.condition(node)?;
        // The integer result is deliberately not computed from unknown text.
        Ok(Value::new(Kind::Integer(None), Vec::new()))
    }
}
