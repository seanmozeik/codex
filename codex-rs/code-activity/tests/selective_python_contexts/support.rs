use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::ConditionalSnapshotPreview;
use codex_code_activity::Language;
use codex_code_activity::SelectiveEditAnalysis;
use codex_code_activity::SelectiveEditDetails;
use codex_code_activity::SelectiveEditInput;
use codex_code_activity::SnapshotEpoch;
use codex_code_activity::SnapshotPreviewInput;
use codex_code_activity::SnapshotReadBinding;
use codex_code_activity::SnapshotReadNewline;
use codex_code_activity::SnapshotTargetBinding;
use codex_code_activity::SnapshotTextProfile;
use codex_code_activity::SnapshotWriteNewline;
use codex_code_activity::SourceReadId;
use codex_code_activity::Target;
use codex_code_activity::TextEncoding;
use codex_code_activity::TrustedReadSnapshot;
use codex_code_activity::TrustedTargetSnapshot;
use codex_code_activity::materialize_snapshot_preview;
use serde_json::Value;
use std::time::Duration;

pub type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub fn analyze(source: &str) -> TestResult<SelectiveEditAnalysis> {
    let input = SelectiveEditInput::from_source(
        "authored-python-contexts".into(),
        Language::Python,
        source,
        None,
        AnalysisLimits::new(32_768, Duration::from_secs(2)),
    )?;
    Ok(Analyzer::new()?.forecast_edits(input))
}

pub fn forecast(source: &str) -> TestResult<Value> {
    Ok(serde_json::to_value(analyze(source)?)?)
}

pub fn prediction(report: &Value, ordinal: usize) -> &Value {
    &report["edits"][ordinal]["details"]["prediction"]
}

pub fn text<'s>(source: &'s str, span: &Value) -> TestResult<&'s str> {
    let start = usize::try_from(span["startByte"].as_u64().ok_or("missing span start")?)?;
    let end = usize::try_from(span["endByte"].as_u64().ok_or("missing span end")?)?;
    Ok(source.get(start..end).ok_or("span is not source text")?)
}

pub struct ReadSnapshot<'a> {
    pub id: usize,
    pub epoch: usize,
    pub path: &'a str,
    pub bytes: &'a [u8],
}

pub fn preview(
    analysis: &SelectiveEditAnalysis,
    ordinal: usize,
    before: Option<&[u8]>,
    read: Option<ReadSnapshot<'_>>,
) -> TestResult<ConditionalSnapshotPreview> {
    let edit = analysis
        .edits()
        .get(ordinal)
        .ok_or("missing selected edit")?;
    let SelectiveEditDetails::Python { prediction, .. } = &edit.details else {
        return Err("expected Python edit".into());
    };
    let target = TrustedTargetSnapshot::new(
        SnapshotTargetBinding::new(
            edit.evidence.source_id,
            SnapshotEpoch::new(prediction.ordinal),
            prediction.target.clone(),
        ),
        before.map(<[u8]>::to_vec),
    );
    let mut input = SnapshotPreviewInput::new(analysis.call_id().clone(), target);
    input.text_profile = SnapshotTextProfile::new(
        SnapshotReadNewline::PythonUniversal,
        SnapshotWriteNewline::Lf,
    );
    if let Some(read) = read {
        input.reads.push(TrustedReadSnapshot::new(
            SnapshotReadBinding::new(
                SourceReadId::new(edit.evidence.source_id, read.id),
                SnapshotEpoch::new(read.epoch),
                Target::Literal {
                    path: read.path.into(),
                    cwd: None,
                },
                TextEncoding::Utf8,
                SnapshotReadNewline::PythonUniversal,
            ),
            read.bytes.to_vec(),
        ));
    }
    Ok(materialize_snapshot_preview(analysis, ordinal, &input)?)
}
