//! Concrete caller policies must agree with each source-declared output policy.
use crate::source_description::SelectiveEditDetails;
use crate::source_description::SelectivePythonOperation;
use crate::source_description::SelectivePythonWriteNewline;
use crate::source_description::SelectiveSnapshotPreview;
use crate::source_description::SelectiveSnapshotReadNewline;
use crate::source_description::SelectiveSnapshotWriteNewline;
use crate::source_description::SelectiveTextEncoding;

pub(super) fn valid(snapshot: &SelectiveSnapshotPreview, details: &SelectiveEditDetails) -> bool {
    let profile = &snapshot.text_profile;
    if profile.runtime_default_encoding.is_some() {
        return false;
    }
    match details {
        SelectiveEditDetails::Python { prediction, .. } => {
            let policy = match &prediction.operation {
                SelectivePythonOperation::Truncate => None,
                SelectivePythonOperation::WriteText {
                    encoding, newline, ..
                } => {
                    if *encoding != SelectiveTextEncoding::Utf8 {
                        return false;
                    }
                    Some(newline.unwrap_or(SelectivePythonWriteNewline::RuntimeDefault))
                }
                SelectivePythonOperation::LogicalWrite(write) => {
                    if write.encoding != SelectiveTextEncoding::Utf8 {
                        return false;
                    }
                    Some(write.newline)
                }
            };
            profile.read_newline != SelectiveSnapshotReadNewline::Unspecified
                && match policy {
                    None => profile.write_newline != SelectiveSnapshotWriteNewline::Unspecified,
                    Some(SelectivePythonWriteNewline::RuntimeDefault) => matches!(
                        profile.write_newline,
                        SelectiveSnapshotWriteNewline::Lf | SelectiveSnapshotWriteNewline::CrLf
                    ),
                    Some(SelectivePythonWriteNewline::Preserve) => {
                        profile.write_newline == SelectiveSnapshotWriteNewline::Preserve
                    }
                    Some(SelectivePythonWriteNewline::Cr) => {
                        profile.write_newline == SelectiveSnapshotWriteNewline::Cr
                    }
                    Some(SelectivePythonWriteNewline::CrLf) => {
                        profile.write_newline == SelectiveSnapshotWriteNewline::CrLf
                    }
                }
        }
        SelectiveEditDetails::JavaScript { .. } => {
            matches!(
                profile.read_newline,
                SelectiveSnapshotReadNewline::Preserve
                    | SelectiveSnapshotReadNewline::SourceDeclared
            ) && profile.write_newline == SelectiveSnapshotWriteNewline::Preserve
        }
        SelectiveEditDetails::Patch { .. } => false,
    }
}
