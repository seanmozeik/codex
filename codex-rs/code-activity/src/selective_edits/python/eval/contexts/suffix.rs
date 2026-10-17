//! Target-only suffix evidence; never evaluate or complete an opaque operation.

use super::HandleId;
use super::HandleState;
use super::Kind;
use super::PythonBarrierReason;
use super::PythonOperation;
use super::Result;
use super::State;
use super::parts;
use crate::selective_edits::python::PythonSuffixTouch;
use tree_sitter::Node;

impl State<'_> {
    pub(super) fn summarize_suffix_touch(
        &mut self,
        id: HandleId,
        stopped: Node<'_>,
        next: Option<Node<'_>>,
    ) -> Result<()> {
        if let Some(touch) = self.suffix_touch(id, stopped, next) {
            self.guard.charge(384)?;
            self.suffix_touches.push(touch);
        }
        Ok(())
    }

    fn suffix_touch(
        &self,
        id: HandleId,
        stopped: Node<'_>,
        next: Option<Node<'_>>,
    ) -> Option<PythonSuffixTouch> {
        let handle = self.context_handles.get(id.0)?;
        if !matches!(handle.state, HandleState::LogicalWritten) {
            return None;
        }
        let flush = self.owned_suffix_call(stopped, id, "flush")?;
        if parts(flush.child_by_field_name("arguments")?)
            .next()
            .is_some()
        {
            return None;
        }
        let barrier = self.plan.barriers.last()?;
        if barrier.reason != PythonBarrierReason::UnsupportedOperation
            || barrier.evidence != self.evidence(flush.child_by_field_name("function")?)
        {
            return None;
        }
        let truncate = self.owned_suffix_call(next?, id, "truncate")?;
        let mut arguments = parts(truncate.child_by_field_name("arguments")?);
        let size = arguments.next()?;
        let spelling = crate::text(size, self.source);
        if arguments.next().is_some()
            || size.kind() != "integer"
            || spelling.is_empty()
            || !spelling.bytes().all(|byte| byte.is_ascii_digit())
            || spelling.parse::<i64>().is_err()
        {
            return None;
        }
        let edit = self.plan.edits.last()?;
        let PythonOperation::LogicalWrite(write) = &edit.operation else {
            return None;
        };
        if write.stream_id.get() != id.0 || write.opened_at != handle.opened_at {
            return None;
        }
        // Conditional on reaching this same-owner truncate, the target is known.
        // Flush success, resulting bytes and subsequent execution remain unknown.
        Some(PythonSuffixTouch {
            edit_ordinal: edit.ordinal,
            write_evidence: edit.evidence.clone(),
            stream_id: write.stream_id,
            opened_at: write.opened_at.clone(),
            flush_evidence: self.evidence(flush),
            truncate_evidence: self.evidence(truncate),
        })
    }

    fn owned_suffix_call<'a>(
        &self,
        statement: Node<'a>,
        id: HandleId,
        method: &str,
    ) -> Option<Node<'a>> {
        if statement.kind() != "expression_statement" {
            return None;
        }
        let mut expressions = parts(statement);
        let call = expressions.next()?;
        if expressions.next().is_some() || call.kind() != "call" {
            return None;
        }
        let function = call.child_by_field_name("function")?;
        if function.kind() != "attribute"
            || crate::text(function.child_by_field_name("attribute")?, self.source) != method
        {
            return None;
        }
        let owner = function.child_by_field_name("object")?;
        if owner.kind() != "identifier" {
            return None;
        }
        let binding = self.bindings.get(crate::text(owner, self.source))?;
        match &binding.value.kind {
            Kind::ContextHandle(receiver) if receiver.0 == id.0 => Some(call),
            _ => None,
        }
    }
}
