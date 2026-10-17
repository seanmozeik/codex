//! Shared local descriptor identity and conditional cursor or EOF transitions.

use super::Assumption;
use super::Dependency;
use super::Fallback;
use super::Kind;
use super::MAX_RECORDS;
use super::Reason;
use super::Recipe;
use super::Result;
use super::Span;
use super::State;
use super::Target;
use super::Use;
use super::Value;
use super::WriteEffect;
use crate::selective_edits::JavaScriptDescriptorPosition as Position;
use tree_sitter::Node;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct DescriptorId(usize);

#[derive(Clone, Copy)]
enum Cursor {
    Known(usize),
    Unknown,
}

#[derive(Clone, Copy)]
enum WriteMode {
    Positioned(Cursor),
    Append,
}

#[derive(Clone)]
pub(super) struct Descriptor {
    target: Target,
    opened_at: Span,
    mode: WriteMode,
    weight: usize,
}

impl State<'_> {
    pub(super) fn open_descriptor(
        &mut self,
        node: Node<'_>,
        path: Node<'_>,
        flags: Node<'_>,
        receiver: Value,
    ) -> Result<Value> {
        let path_value = self.expression(path, Use::Value)?;
        let flags_value = self.expression(flags, Use::Value)?;
        let Some(flags) = self.constant(&flags_value)? else {
            return self.halt(node, Reason::UnsupportedOperation);
        };
        if !matches!(flags.as_str(), "w" | "w+" | "r+" | "a") {
            return self.halt(node, Reason::UnsupportedOperation);
        }
        let target = self.target(path, &path_value)?;
        if self.next_descriptor >= MAX_RECORDS {
            return Err(Fallback::SourceLimit.into());
        }
        let id = DescriptorId(self.next_descriptor);
        self.next_descriptor += 1;
        let opened_at = self.evidence(node);
        let weight = receiver.weight + path_value.weight + flags_value.weight + 256;
        let mut dependencies = receiver.dependencies;
        self.merge(&mut dependencies, path_value.dependencies)?;
        self.merge(&mut dependencies, flags_value.dependencies)?;
        self.dependency(
            &mut dependencies,
            Dependency::Descriptor {
                descriptor_id: id.0,
                opened_at: opened_at.clone(),
            },
        )?;
        self.charge(weight)?;
        self.descriptor_assumption(Assumption::RegularDescriptorFiles);
        if matches!(flags.as_str(), "w" | "w+") {
            self.emit(
                node,
                target.clone(),
                Recipe::Literal {
                    value: String::new(),
                    evidence: opened_at.clone(),
                },
                WriteEffect::Truncate {
                    descriptor_id: id.0,
                    opened_at: opened_at.clone(),
                },
                dependencies.clone(),
            )?;
        } else {
            self.condition(node)?;
        }
        self.descriptors.insert(
            id,
            Descriptor {
                target,
                opened_at,
                mode: if flags == "a" {
                    WriteMode::Append
                } else {
                    WriteMode::Positioned(Cursor::Known(0))
                },
                weight,
            },
        );
        self.value(Kind::Descriptor(id), dependencies, weight, 0)
    }

    fn owned_descriptor(
        &mut self,
        node: Node<'_>,
    ) -> Result<(DescriptorId, Descriptor, Vec<Dependency>)> {
        if node.kind() != "identifier" {
            return self.halt(node, Reason::UnsupportedOperation);
        }
        let value = self.lookup(node)?;
        let Kind::Descriptor(id) = value.kind else {
            return self.halt(node, Reason::UnsupportedOperation);
        };
        let Some(weight) = self
            .descriptors
            .get(&id)
            .map(|descriptor| descriptor.weight)
        else {
            return self.halt(node, Reason::UnsupportedOperation);
        };
        self.charge(weight)?;
        let descriptor = self
            .descriptors
            .get(&id)
            .cloned()
            .ok_or(Fallback::UnsupportedSource)?;
        Ok((id, descriptor, value.dependencies))
    }

    pub(super) fn write_descriptor(
        &mut self,
        node: Node<'_>,
        arguments: &[Node<'_>],
        receiver: Value,
    ) -> Result<Value> {
        let [owner, data, rest @ ..] = arguments else {
            return self.halt(node, Reason::UnsupportedOperation);
        };
        let (id, mut descriptor, owner_dependencies) = self.owned_descriptor(*owner)?;
        let data_value = self.expression(*data, Use::Value)?;
        let mut dependencies = receiver.dependencies;
        self.merge(&mut dependencies, owner_dependencies)?;
        let effect = self.descriptor_effect(
            node,
            id,
            &mut descriptor,
            rest.first().copied(),
            &data_value,
            &mut dependencies,
        )?;
        if let Some(encoding) = rest.get(1) {
            let encoding_value = self.encoding(*encoding)?;
            self.merge(&mut dependencies, encoding_value.dependencies)?;
        }
        let Kind::Text(content) = data_value.kind else {
            return self.halt(*data, Reason::UnsupportedOperation);
        };
        self.merge(&mut dependencies, data_value.dependencies)?;
        self.descriptor_assumption(Assumption::CompleteDescriptorWrites);
        self.emit(
            node,
            descriptor.target.clone(),
            content,
            effect,
            dependencies,
        )?;
        self.descriptors.insert(id, descriptor);
        self.value(Kind::Discarded, Vec::new(), 64, 0)
    }

    fn descriptor_effect(
        &mut self,
        call: Node<'_>,
        id: DescriptorId,
        descriptor: &mut Descriptor,
        argument: Option<Node<'_>>,
        data: &Value,
        dependencies: &mut Vec<Dependency>,
    ) -> Result<WriteEffect> {
        let cursor = match descriptor.mode {
            WriteMode::Append => {
                if let Some(node) = argument {
                    let value = self.expression(node, Use::Value)?;
                    self.merge(dependencies, value.dependencies)?;
                    if !matches!(value.kind, Kind::Null) {
                        // Numeric append positions differ across platforms.
                        return self.halt(node, Reason::UnsupportedOperation);
                    }
                }
                return Ok(WriteEffect::DescriptorAppend {
                    descriptor_id: id.0,
                    opened_at: descriptor.opened_at.clone(),
                });
            }
            WriteMode::Positioned(cursor) => cursor,
        };
        let (position, byte_offset) =
            self.descriptor_position(call, argument, cursor, dependencies)?;
        if position == Position::Cursor {
            let next = match self.constant(data)? {
                Some(text) => byte_offset
                    .checked_add(text.len())
                    .map_or(Cursor::Unknown, Cursor::Known),
                None => Cursor::Unknown,
            };
            descriptor.mode = WriteMode::Positioned(next);
        }
        Ok(WriteEffect::DescriptorWrite {
            descriptor_id: id.0,
            opened_at: descriptor.opened_at.clone(),
            byte_offset,
            position,
        })
    }

    fn descriptor_position(
        &mut self,
        call: Node<'_>,
        node: Option<Node<'_>>,
        cursor: Cursor,
        dependencies: &mut Vec<Dependency>,
    ) -> Result<(Position, usize)> {
        if let Some(node) = node {
            let value = self.expression(node, Use::Value)?;
            self.merge(dependencies, value.dependencies)?;
            match value.kind {
                Kind::Integer(offset) => return Ok((Position::Absolute, offset)),
                Kind::Null => {}
                _ => return self.halt(node, Reason::UnsupportedOperation),
            }
        }
        match cursor {
            Cursor::Known(offset) => Ok((Position::Cursor, offset)),
            Cursor::Unknown => self.halt(call, Reason::UnsupportedOperation),
        }
    }

    pub(super) fn close_descriptor(&mut self, node: Node<'_>, owner: Node<'_>) -> Result<Value> {
        let (id, _, _) = self.owned_descriptor(owner)?;
        self.condition(node)?;
        self.descriptors.remove(&id);
        self.value(Kind::Discarded, Vec::new(), 64, 0)
    }

    fn descriptor_assumption(&mut self, assumption: Assumption) {
        if !self.plan.assumptions.contains(&assumption) {
            self.plan.assumptions.push(assumption);
        }
    }
}
