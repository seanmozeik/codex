//! Canonical Node calls and bounded immutable primitive-string operations.

use super::Dependency;
use super::Fallback;
use super::FilesystemMethod;
use super::Kind;
use super::MAX_RECORDS;
use super::Reason;
use super::Recipe;
use super::Result;
use super::State;
use super::Target;
use super::Use;
use super::Value;
use super::WriteEffect;
use super::syntax;
use tree_sitter::Node;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReplaceMode {
    First,
    All,
}

impl State<'_> {
    pub(super) fn call(&mut self, node: Node<'_>, usage: Use) -> Result<Value> {
        let (callee, arguments) = syntax::call(node, self.source)?;
        if callee.kind() == "identifier" && crate::text(callee, self.source) == "require" {
            let [module] = arguments.as_slice() else {
                return self.halt(node, Reason::UnmodeledCall);
            };
            if module.kind() != "string"
                || !matches!(
                    syntax::literal(*module, self.source)?.as_str(),
                    "fs" | "node:fs"
                )
            {
                return self.halt(node, Reason::UnmodeledCall);
            }
            self.condition(node)?;
            return self.value(Kind::Filesystem, Vec::new(), 128, 0);
        }
        if callee.kind() == "identifier" {
            if !self.declared.contains_key(crate::text(callee, self.source)) {
                return self.halt(node, Reason::UnmodeledCall);
            }
            let receiver = self.expression(callee, Use::Value)?;
            let Kind::FilesystemFunction(method) = receiver.kind else {
                return self.halt(node, Reason::UnmodeledCall);
            };
            return self.filesystem(node, method, &arguments, receiver, usage);
        }
        if callee.kind() != "member_expression" {
            return self.halt(node, Reason::UnmodeledCall);
        }
        let (object, method) = syntax::member(callee, self.source)?;
        let receiver = self.expression(object, Use::Value)?;
        match &receiver.kind {
            Kind::Filesystem => match FilesystemMethod::from_name(method) {
                Some(method) => self.filesystem(node, method, &arguments, receiver, usage),
                None => self.halt(node, Reason::UnmodeledCall),
            },
            Kind::Text(_) if method == "replace" => {
                self.replace(node, &arguments, receiver, ReplaceMode::First)
            }
            Kind::Text(_) if method == "replaceAll" => {
                self.replace(node, &arguments, receiver, ReplaceMode::All)
            }
            Kind::Text(_) if method == "slice" => self.slice(node, &arguments, receiver),
            _ => self.halt(node, Reason::UnmodeledCall),
        }
    }

    fn filesystem(
        &mut self,
        node: Node<'_>,
        method: FilesystemMethod,
        arguments: &[Node<'_>],
        receiver: Value,
        usage: Use,
    ) -> Result<Value> {
        match (method, arguments) {
            (FilesystemMethod::Read, [path, encoding]) => {
                self.read(node, *path, *encoding, receiver)
            }
            (FilesystemMethod::Write, [path, data] | [path, data, _]) if usage == Use::Discard => {
                self.write(node, *path, *data, arguments.get(2).copied(), receiver)
            }
            (FilesystemMethod::Open, [path, flags]) if usage == Use::InitializeConst => {
                self.open_descriptor(node, *path, *flags, receiver)
            }
            (FilesystemMethod::WriteDescriptor, [_, _] | [_, _, _] | [_, _, _, _])
                if usage == Use::Discard =>
            {
                self.write_descriptor(node, arguments, receiver)
            }
            (FilesystemMethod::Close, [descriptor]) if usage == Use::Discard => {
                self.close_descriptor(node, *descriptor)
            }
            _ => self.halt(node, Reason::UnmodeledCall),
        }
    }

    pub(super) fn target(&mut self, node: Node<'_>, value: &Value) -> Result<Target> {
        let Some(path) = self.constant(value)? else {
            return self.halt(node, Reason::UnsupportedOperation);
        };
        if path.is_empty() || path.contains('\0') {
            return self.halt(node, Reason::UnsupportedOperation);
        }
        self.charge(path.len() + self.cwd.map_or(0, str::len) + 128)?;
        Ok(Target::Literal {
            path,
            cwd: self.cwd.map(str::to_owned),
        })
    }

    pub(super) fn encoding(&mut self, node: Node<'_>) -> Result<Value> {
        let value = self.expression(node, Use::Value)?;
        if self.constant(&value)?.as_deref() != Some("utf8") {
            return self.halt(node, Reason::UnsupportedOperation);
        }
        Ok(value)
    }

    fn read(
        &mut self,
        node: Node<'_>,
        path: Node<'_>,
        encoding: Node<'_>,
        receiver: Value,
    ) -> Result<Value> {
        let path_value = self.expression(path, Use::Value)?;
        let encoding_value = self.encoding(encoding)?;
        let target = self.target(path, &path_value)?;
        if self.next_read >= MAX_RECORDS {
            return Err(Fallback::SourceLimit.into());
        }
        let read_id = self.next_read;
        self.next_read += 1;
        let mut dependencies = receiver.dependencies;
        self.merge(&mut dependencies, path_value.dependencies)?;
        self.merge(&mut dependencies, encoding_value.dependencies)?;
        self.dependency(
            &mut dependencies,
            Dependency::ReadValue {
                read_id,
                evidence: self.evidence(node),
            },
        )?;
        self.dependency(
            &mut dependencies,
            Dependency::FilesystemEpoch {
                epoch: self.plan.edits.len(),
            },
        )?;
        self.condition(node)?;
        self.value(
            Kind::Text(Recipe::ReadUtf8 {
                read_id,
                target,
                evidence: self.evidence(node),
                filesystem_epoch: self.plan.edits.len(),
            }),
            dependencies,
            receiver.weight
                + path_value.weight
                + encoding_value.weight
                + self.cwd.map_or(0, str::len)
                + 256,
            1,
        )
    }

    fn write(
        &mut self,
        node: Node<'_>,
        path: Node<'_>,
        data: Node<'_>,
        encoding: Option<Node<'_>>,
        receiver: Value,
    ) -> Result<Value> {
        let path_value = self.expression(path, Use::Value)?;
        let data_value = self.expression(data, Use::Value)?;
        let encoding_value = encoding
            .map(|encoding| self.encoding(encoding))
            .transpose()?;
        let target = self.target(path, &path_value)?;
        let Kind::Text(content) = data_value.kind else {
            return self.halt(data, Reason::UnsupportedOperation);
        };
        let mut dependencies = receiver.dependencies;
        self.merge(&mut dependencies, path_value.dependencies)?;
        self.merge(&mut dependencies, data_value.dependencies)?;
        if let Some(value) = encoding_value {
            self.merge(&mut dependencies, value.dependencies)?;
        }
        self.emit(node, target, content, WriteEffect::Overwrite, dependencies)?;
        self.value(Kind::Discarded, Vec::new(), 64, 0)
    }

    fn slice(&mut self, node: Node<'_>, arguments: &[Node<'_>], receiver: Value) -> Result<Value> {
        let start = match arguments {
            [start] | [start, _] => *start,
            _ => return self.halt(node, Reason::UnmodeledCall),
        };
        let start_value = self.expression(start, Use::Value)?;
        let end_value = arguments
            .get(1)
            .map(|end| self.expression(*end, Use::Value))
            .transpose()?;
        let Kind::Integer(start) = start_value.kind else {
            return self.halt(node, Reason::UnsupportedOperation);
        };
        let end = match end_value.as_ref().map(|value| &value.kind) {
            None => None,
            Some(Kind::Integer(end)) => Some(*end),
            _ => return self.halt(node, Reason::UnsupportedOperation),
        };
        let Kind::Text(input) = receiver.kind else {
            return self.halt(node, Reason::UnsupportedOperation);
        };
        let mut dependencies = receiver.dependencies;
        self.merge(&mut dependencies, start_value.dependencies)?;
        let mut weight = receiver.weight + start_value.weight + 128;
        if let Some(value) = end_value {
            weight += value.weight;
            self.merge(&mut dependencies, value.dependencies)?;
        }
        self.value(
            Kind::Text(Recipe::Slice {
                input: Box::new(input),
                start,
                end,
                evidence: self.evidence(node),
            }),
            dependencies,
            weight,
            receiver.depth + 1,
        )
    }

    fn replace(
        &mut self,
        node: Node<'_>,
        arguments: &[Node<'_>],
        receiver: Value,
        mode: ReplaceMode,
    ) -> Result<Value> {
        let [old, new] = arguments else {
            return self.halt(node, Reason::UnmodeledCall);
        };
        let old_value = self.expression(*old, Use::Value)?;
        let new_value = self.expression(*new, Use::Value)?;
        let (Some(old), Some(new)) = (self.constant(&old_value)?, self.constant(&new_value)?)
        else {
            return self.halt(node, Reason::UnsupportedOperation);
        };
        if new.contains('$') || (mode == ReplaceMode::All && old.is_empty()) {
            return self.halt(node, Reason::UnsupportedOperation);
        }
        let Kind::Text(input) = receiver.kind else {
            return self.halt(node, Reason::UnsupportedOperation);
        };
        let mut dependencies = receiver.dependencies;
        self.merge(&mut dependencies, old_value.dependencies)?;
        self.merge(&mut dependencies, new_value.dependencies)?;
        let input = Box::new(input);
        let evidence = self.evidence(node);
        let recipe = match mode {
            ReplaceMode::First => Recipe::ReplaceFirst {
                input,
                old,
                new,
                evidence,
            },
            ReplaceMode::All => Recipe::ReplaceAll {
                input,
                old,
                new,
                evidence,
            },
        };
        self.value(
            Kind::Text(recipe),
            dependencies,
            receiver.weight + old_value.weight + new_value.weight + 128,
            receiver.depth + 1,
        )
    }
}
