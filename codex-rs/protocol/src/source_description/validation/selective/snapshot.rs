//! Bounded identity/disclosure checks, not authentication or execution proof.

mod diff;
mod logical;
mod profile;

use super::ReadIdentity;
use super::display_text;
use crate::source_description::SelectiveEditDetails;
use crate::source_description::SelectivePredictedEdit;
use crate::source_description::SelectivePythonOperation;
use crate::source_description::SelectiveSnapshotPreview;
use crate::source_description::SelectiveSnapshotReadNewline;
use crate::source_description::SelectiveTarget;
use crate::source_description::SelectiveTextEncoding;
use crate::source_description::selective::snapshot::MAX_SNAPSHOT_DIFF_BYTES;
use crate::source_description::selective::snapshot::MAX_SNAPSHOT_FILE_BYTES;
use crate::source_description::selective::snapshot::MAX_SNAPSHOT_PAYLOAD_BYTES;
use crate::source_description::selective::snapshot::MAX_SNAPSHOT_PREVIEWS;
use std::collections::BTreeMap;
use std::collections::BTreeSet;

#[derive(Default)]
pub(super) struct Budget {
    count: usize,
    bytes: usize,
}

impl Budget {
    fn text(&mut self, text: &str, limit: usize, may_display: &impl Fn(&str) -> bool) -> bool {
        self.bytes = self.bytes.saturating_add(text.len());
        text.len() <= limit
            && self.bytes <= MAX_SNAPSHOT_PAYLOAD_BYTES
            && display_text(text, may_display)
    }
}

pub(super) fn valid(
    edit: &SelectivePredictedEdit,
    previous: &[SelectivePredictedEdit],
    reads: &BTreeMap<(usize, usize), ReadIdentity>,
    budget: &mut Budget,
    may_display: &impl Fn(&str) -> bool,
) -> bool {
    let Some(snapshot) = &edit.snapshot else {
        return true;
    };
    let Some((epoch, target)) = operation(edit) else {
        return false;
    };
    budget.count = budget.count.saturating_add(1);
    if budget.count > MAX_SNAPSHOT_PREVIEWS
        || snapshot.source_id != edit.evidence.source_id
        || snapshot.filesystem_epoch != epoch
        || !edit.predecessor_conditions.is_empty()
        || snapshot
            .before
            .as_ref()
            .is_some_and(|before| !budget.text(before, MAX_SNAPSHOT_FILE_BYTES, may_display))
        || !budget.text(&snapshot.after, MAX_SNAPSHOT_FILE_BYTES, may_display)
        || !budget.text(&snapshot.unified_diff, MAX_SNAPSHOT_DIFF_BYTES, may_display)
        || [
            &snapshot.environment_id,
            &snapshot.resolved_cwd,
            &snapshot.resolved_target,
        ]
        .into_iter()
        .any(|text| text.is_empty() || !budget.text(text, 32_768, may_display))
        || !profile::valid(snapshot, &edit.details)
        || !logical::valid(snapshot, &edit.details, budget, may_display)
        || !replay_valid(snapshot, target, previous)
        || !diff::matches_endpoints(snapshot)
        || snapshot.read_bindings.len() > MAX_SNAPSHOT_PREVIEWS
    {
        return false;
    }
    let mut seen = BTreeSet::new();
    snapshot.read_bindings.iter().all(|binding| {
        let key = (binding.source_id, binding.read_id);
        let Some((_, read_target, read_epoch, encoding, newline)) = reads.get(&key) else {
            return false;
        };
        binding.source_id == snapshot.source_id
            && seen.insert(key)
            && binding.filesystem_epoch == *read_epoch
            && *read_epoch <= epoch
            && binding.target == *read_target
            && binding.encoding == *encoding
            && binding.encoding == SelectiveTextEncoding::Utf8
            && binding.newline == *newline
            && (snapshot.text_profile.read_newline == SelectiveSnapshotReadNewline::SourceDeclared
                || binding.newline == snapshot.text_profile.read_newline)
            && budget.text(&binding.target.path, 32_768, may_display)
            && binding
                .target
                .cwd
                .as_ref()
                .is_none_or(|cwd| budget.text(cwd, 32_768, may_display))
    })
}

fn operation(edit: &SelectivePredictedEdit) -> Option<(usize, &SelectiveTarget)> {
    match &edit.details {
        SelectiveEditDetails::Python { prediction, .. } => Some((
            match &prediction.operation {
                SelectivePythonOperation::LogicalWrite(write) => write.base_filesystem_epoch,
                _ => prediction.ordinal,
            },
            &prediction.target,
        )),
        SelectiveEditDetails::JavaScript { prediction, .. } => {
            Some((prediction.ordinal, &prediction.target))
        }
        SelectiveEditDetails::Patch { .. } => None,
    }
}

fn replay_valid(
    snapshot: &SelectiveSnapshotPreview,
    target: &SelectiveTarget,
    previous: &[SelectivePredictedEdit],
) -> bool {
    let mut replay = snapshot.replay_ordinals.iter();
    let mut last = None;
    let mut count = 0;
    for edit in previous
        .iter()
        .filter(|edit| edit.evidence.source_id == snapshot.source_id)
    {
        let Some((epoch, previous_target)) = operation(edit) else {
            return false;
        };
        let Some(preview) = &edit.snapshot else {
            return false;
        };
        if preview.scope != crate::source_description::SelectiveSnapshotScope::OperationPoint
            || preview.logical_stream.is_some()
            || replay.next() != Some(&edit.ordinal)
            || epoch != count
            || previous_target != target
            || preview.environment_id != snapshot.environment_id
            || preview.resolved_cwd != snapshot.resolved_cwd
            || preview.resolved_target != snapshot.resolved_target
        {
            return false;
        }
        count += 1;
        last = Some(preview);
    }
    count == snapshot.filesystem_epoch
        && replay.next().is_none()
        && last.is_none_or(|previous| snapshot.before.as_ref() == Some(&previous.after))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod logical_tests;
