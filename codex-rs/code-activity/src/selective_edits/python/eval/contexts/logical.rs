//! Owned stream state transitions; logical bytes do not advance disk epochs.

use super::Argument;
use super::Fallback;
use super::HandleId;
use super::HandleState;
use super::Kind;
use super::Method;
use super::Mode;
use super::Node;
use super::PythonBarrierReason;
use super::PythonDependency;
use super::PythonOperation;
use super::PythonStreamMode;
use super::PythonStreamPosition;
use super::Result;
use super::State;
use super::TextEncoding;
use super::TextRecipe;
use super::Value;
use super::field;
use super::target_weight;
use super::unparenthesized;
use crate::selective_edits::PythonLogicalWrite;
use crate::selective_edits::PythonStreamId;
use crate::selective_edits::PythonWriteResultCondition;

impl State<'_> {
    pub(super) fn read_context(
        &mut self,
        node: Node<'_>,
        id: HandleId,
        arguments: Vec<Argument>,
        dependencies: Vec<PythonDependency>,
    ) -> Result<Value> {
        let read_id = self.next_read;
        let read_evidence = self.evidence(node);
        let handle = self
            .context_handles
            .get_mut(id.0)
            .ok_or(Fallback::UnsupportedSource)?;
        let logical = matches!(
            &handle.state,
            HandleState::Logical {
                mode: PythonStreamMode::UpdateExisting,
                position: PythonStreamPosition::InitialZero
            }
        );
        if !logical && !matches!(&handle.state, HandleState::Readable) {
            return self.halt(node, PythonBarrierReason::UnsupportedOperation);
        }
        self.guard.charge(target_weight(&handle.target))?;
        handle.state = if logical {
            HandleState::LogicalRead {
                read_id,
                read_evidence,
            }
        } else {
            HandleState::Read
        };
        let receiver = Value::new(
            Kind::Handle {
                target: handle.target.clone(),
                mode: Mode::Read,
                encoding: TextEncoding::Utf8,
                newline: handle.newline,
            },
            Vec::new(),
        );
        self.read(node, receiver, Method::OpenRead, arguments, dependencies)
    }

    pub(super) fn seek_context(
        &mut self,
        node: Node<'_>,
        id: HandleId,
        arguments: Vec<Argument>,
    ) -> Result<Value> {
        if arguments.len() != 1
            || arguments[0].name.is_some()
            || !matches!(&arguments[0].value.kind, Kind::Integer(Some(0)))
            || crate::text(
                unparenthesized(
                    field(node, "arguments")?
                        .named_child(0)
                        .ok_or(Fallback::UnsupportedSource)?,
                )?,
                self.source,
            ) != "0"
        {
            return self.halt(node, PythonBarrierReason::UnsupportedOperation);
        }
        let seek_evidence = self.evidence(node);
        let handle = self
            .context_handles
            .get_mut(id.0)
            .ok_or(Fallback::UnsupportedSource)?;
        let HandleState::LogicalRead {
            read_id,
            read_evidence,
        } = &handle.state
        else {
            return self.halt(node, PythonBarrierReason::UnsupportedOperation);
        };
        handle.state = HandleState::Logical {
            mode: PythonStreamMode::UpdateExisting,
            position: PythonStreamPosition::RewoundZero {
                read_id: *read_id,
                read_evidence: read_evidence.clone(),
                seek_evidence,
            },
        };
        self.condition(node)?;
        Ok(Value::new(Kind::Integer(Some(0)), Vec::new()))
    }

    pub(super) fn logical_write(
        &mut self,
        node: Node<'_>,
        id: HandleId,
        content: TextRecipe,
        mut dependencies: Vec<PythonDependency>,
    ) -> Result<()> {
        let handle = self
            .context_handles
            .get(id.0)
            .ok_or(Fallback::UnsupportedSource)?;
        let HandleState::Logical { mode, position } = &handle.state else {
            return self.halt(node, PythonBarrierReason::UnsupportedOperation);
        };
        self.guard.charge(target_weight(&handle.target) + 256)?;
        let target = handle.target.clone();
        let operation = PythonLogicalWrite {
            stream_id: PythonStreamId::new(id.0),
            opened_at: handle.opened_at.clone(),
            mode: *mode,
            position: position.clone(),
            base_filesystem_epoch: handle.base_epoch,
            stream_revision: 0,
            content,
            encoding: TextEncoding::Utf8,
            newline: handle.newline.ok_or(Fallback::UnsupportedSource)?,
            required_result: PythonWriteResultCondition::FullCharacterCount,
        };
        if let PythonStreamPosition::RewoundZero {
            read_id,
            read_evidence,
            ..
        } = &operation.position
        {
            self.dependency(
                &mut dependencies,
                PythonDependency::ReadValue {
                    read_id: *read_id,
                    evidence: read_evidence.clone(),
                },
            )?;
        }
        self.emit(
            node,
            target,
            PythonOperation::LogicalWrite(operation),
            dependencies,
        )?;
        self.context_handles
            .get_mut(id.0)
            .ok_or(Fallback::UnsupportedSource)?
            .state = HandleState::LogicalWritten;
        self.logical_endpoint = true;
        self.condition(node)
    }
}
