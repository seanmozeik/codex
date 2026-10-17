//! Caller-authored identities; never infer a missing binding from predictions.

use super::Case;
use codex_code_activity as activity;
use serde::Deserialize;

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
enum DeclaredEncoding {
    Utf8,
    RuntimeDefault,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ReadInput {
    source_id: usize,
    read_id: usize,
    filesystem_epoch: usize,
    path: String,
    cwd: Option<String>,
    encoding: DeclaredEncoding,
    newline: activity::SnapshotReadNewline,
    bytes: Vec<u8>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct TargetInput {
    pub(super) ordinal: usize,
    source_id: usize,
    filesystem_epoch: usize,
    path: String,
    cwd: Option<String>,
    bytes: Option<Vec<u8>>,
    #[serde(default)]
    logical_stream: Option<LogicalInput>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LogicalInput {
    source_id: usize,
    stream_id: usize,
    opened_at: SpanInput,
    base_filesystem_epoch: usize,
    revision: usize,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SpanInput {
    source_id: usize,
    start_byte: usize,
    end_byte: usize,
    start_line: usize,
    end_line: usize,
    start_column: usize,
}

pub(super) enum BoundInput {
    Physical(activity::SnapshotPreviewInput),
    Logical(activity::LogicalStreamPreviewInput),
}

/// Binding kinds select the projection before any identity or byte validation.
/// An invalid logical binding therefore cannot fall back to a physical claim.
pub(super) fn requested_endpoint(targets: &[TargetInput]) -> activity::PythonWriteEndpoint {
    if targets.iter().any(|target| target.logical_stream.is_some()) {
        activity::PythonWriteEndpoint::LogicalAcceptance
    } else {
        activity::PythonWriteEndpoint::ContextCloseWhenAvailable
    }
}

impl BoundInput {
    pub(super) fn materialize(
        &self,
        analysis: &activity::SelectiveEditAnalysis,
        ordinal: usize,
    ) -> Result<activity::ConditionalSnapshotPreview, activity::SnapshotPreviewError> {
        match self {
            Self::Physical(input) => {
                activity::materialize_snapshot_preview(analysis, ordinal, input)
            }
            Self::Logical(input) => {
                activity::materialize_logical_stream_preview(analysis, ordinal, input)
            }
        }
    }
}

pub(super) fn bind(
    case: &Case,
    analysis: &activity::SelectiveEditAnalysis,
    target: &TargetInput,
) -> Result<BoundInput, &'static str> {
    let before = activity::TrustedTargetSnapshot::new(
        activity::SnapshotTargetBinding::new(
            source_id(analysis, target.source_id)?,
            activity::SnapshotEpoch::new(target.filesystem_epoch),
            activity::Target::Literal {
                path: target.path.clone(),
                cwd: target.cwd.clone(),
            },
        ),
        target.bytes.clone(),
    );
    let mut input = activity::SnapshotPreviewInput::new(case.snapshot_call_id.clone(), before);
    input.text_profile = case.text_profile;
    for read in &case.reads {
        input.reads.push(activity::TrustedReadSnapshot::new(
            activity::SnapshotReadBinding::new(
                activity::SourceReadId::new(source_id(analysis, read.source_id)?, read.read_id),
                activity::SnapshotEpoch::new(read.filesystem_epoch),
                activity::Target::Literal {
                    path: read.path.clone(),
                    cwd: read.cwd.clone(),
                },
                match read.encoding {
                    DeclaredEncoding::Utf8 => activity::TextEncoding::Utf8,
                    DeclaredEncoding::RuntimeDefault => activity::TextEncoding::RuntimeDefault,
                },
                read.newline,
            ),
            read.bytes.clone(),
        ));
    }
    let Some(logical) = &target.logical_stream else {
        return Ok(BoundInput::Physical(input));
    };
    let opened = &logical.opened_at;
    let opened_at = activity::Span::new(
        source_id(analysis, opened.source_id)?,
        opened.start_byte,
        opened.end_byte,
        opened.start_line,
        opened.end_line,
        opened.start_column,
    );
    Ok(BoundInput::Logical(
        activity::LogicalStreamPreviewInput::new(
            input,
            activity::LogicalStreamBinding::new(
                source_id(analysis, logical.source_id)?,
                activity::PythonStreamId::new(logical.stream_id),
                opened_at,
                activity::SnapshotEpoch::new(logical.base_filesystem_epoch),
                logical.revision,
            ),
        ),
    ))
}

fn source_id(
    analysis: &activity::SelectiveEditAnalysis,
    index: usize,
) -> Result<activity::SourceId, &'static str> {
    analysis
        .sources()
        .iter()
        .find(|source| source.id.index() == index)
        .map(|source| source.id)
        .ok_or("snapshot names a source outside this analysis")
}
