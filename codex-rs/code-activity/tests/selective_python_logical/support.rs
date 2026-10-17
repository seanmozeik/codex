use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::Language;
use codex_code_activity::LogicalStreamBinding;
use codex_code_activity::LogicalStreamPreviewInput;
use codex_code_activity::PythonOperation;
use codex_code_activity::SelectiveEditAnalysis;
use codex_code_activity::SelectiveEditDetails;
use codex_code_activity::SelectiveEditInput;
use codex_code_activity::SnapshotEpoch;
use codex_code_activity::SnapshotPreviewInput;
use codex_code_activity::SnapshotReadNewline;
use codex_code_activity::SnapshotTargetBinding;
use codex_code_activity::SnapshotTextProfile;
use codex_code_activity::SnapshotWriteNewline;
use codex_code_activity::TrustedTargetSnapshot;
use std::time::Duration;

pub type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub fn analyze(source: &str) -> TestResult<SelectiveEditAnalysis> {
    let input = SelectiveEditInput::from_source(
        "authored-logical".into(),
        Language::Python,
        source,
        None,
        AnalysisLimits::new(32_768, Duration::from_secs(2)),
    )?;
    Ok(Analyzer::new()?.forecast_edits(input))
}

pub fn input(
    analysis: &SelectiveEditAnalysis,
    ordinal: usize,
    bytes: Option<&[u8]>,
) -> TestResult<LogicalStreamPreviewInput> {
    let edit = analysis.edits().get(ordinal).ok_or("missing edit")?;
    let SelectiveEditDetails::Python { prediction, .. } = &edit.details else {
        return Err("not Python".into());
    };
    let PythonOperation::LogicalWrite(write) = &prediction.operation else {
        return Err("not logical".into());
    };
    let epoch = SnapshotEpoch::new(write.base_filesystem_epoch);
    let before = TrustedTargetSnapshot::new(
        SnapshotTargetBinding::new(edit.evidence.source_id, epoch, prediction.target.clone()),
        bytes.map(<[u8]>::to_vec),
    );
    let mut snapshot = SnapshotPreviewInput::new(analysis.call_id().clone(), before);
    snapshot.text_profile = SnapshotTextProfile::new(
        SnapshotReadNewline::SourceDeclared,
        SnapshotWriteNewline::Preserve,
    );
    Ok(LogicalStreamPreviewInput::new(
        snapshot,
        LogicalStreamBinding::new(
            edit.evidence.source_id,
            write.stream_id,
            write.opened_at.clone(),
            epoch,
            0,
        ),
    ))
}
