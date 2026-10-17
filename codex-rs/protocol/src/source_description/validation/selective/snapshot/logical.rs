//! Logical overlays are structurally checked but never assert durable file bytes.
use super::Budget;
use super::MAX_SNAPSHOT_FILE_BYTES;
use crate::source_description::SelectiveEditDetails;
use crate::source_description::SelectivePythonOperation;
use crate::source_description::SelectivePythonStreamMode;
use crate::source_description::SelectiveSnapshotPreview;
use crate::source_description::SelectiveSnapshotScope;
use crate::source_description::SelectiveSnapshotWriteNewline;

pub(super) fn valid(
    snapshot: &SelectiveSnapshotPreview,
    details: &SelectiveEditDetails,
    budget: &mut Budget,
    may_display: &impl Fn(&str) -> bool,
) -> bool {
    let write = match details {
        SelectiveEditDetails::Python { prediction, .. } => match &prediction.operation {
            SelectivePythonOperation::LogicalWrite(write) => Some(write),
            _ => None,
        },
        _ => None,
    };
    let Some(write) = write else {
        return snapshot.scope == SelectiveSnapshotScope::OperationPoint
            && snapshot.logical_stream.is_none();
    };
    let Some(metadata) = &snapshot.logical_stream else {
        return false;
    };
    let Some(before) = &snapshot.before else {
        return false;
    };
    if snapshot.scope != SelectiveSnapshotScope::LogicalStream
        || metadata.binding.source_id != snapshot.source_id
        || metadata.binding.stream_id != write.stream_id
        || metadata.binding.opened_at != write.opened_at
        || metadata.binding.base_filesystem_epoch != write.base_filesystem_epoch
        || metadata.binding.base_filesystem_epoch != snapshot.filesystem_epoch
        || metadata.binding.revision != write.stream_revision
        || metadata.binding.revision != 0
        || metadata.revision_after != 1
        || metadata.mode != write.mode
        || metadata.position != write.position
        || (write.mode == SelectivePythonStreamMode::TruncateWrite && !before.is_empty())
        || metadata.payload_bytes.len() > MAX_SNAPSHOT_FILE_BYTES
    {
        return false;
    }
    let Ok(payload) = std::str::from_utf8(&metadata.payload_bytes) else {
        return false;
    };
    if !budget.text(payload, MAX_SNAPSHOT_FILE_BYTES, may_display) {
        return false;
    }
    // CRLF translation adds one CR per source LF. Existing source CR remains,
    // including CR immediately before an LF; do not normalize CRCRLF pairs.
    let mut characters = payload.chars().count();
    if snapshot.text_profile.write_newline == SelectiveSnapshotWriteNewline::CrLf {
        let bytes = payload.as_bytes();
        for (index, byte) in bytes.iter().enumerate() {
            if *byte == b'\n' {
                if index == 0 || bytes[index - 1] != b'\r' {
                    return false;
                }
                characters -= 1;
            }
        }
    } else if snapshot.text_profile.write_newline == SelectiveSnapshotWriteNewline::Cr
        && payload.contains('\n')
    {
        return false;
    }
    if metadata.accepted_characters_required != characters {
        return false;
    }
    let suffix_start = metadata.payload_bytes.len().min(before.len());
    before.is_char_boundary(suffix_start)
        && snapshot.after.len() == metadata.payload_bytes.len() + before.len() - suffix_start
        && snapshot
            .after
            .as_bytes()
            .starts_with(&metadata.payload_bytes)
        && snapshot
            .after
            .as_bytes()
            .get(metadata.payload_bytes.len()..)
            == before.as_bytes().get(suffix_start..)
}
