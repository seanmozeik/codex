use codex_code_activity::ActivityError;
use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::ConditionalSnapshotPreview;
use codex_code_activity::Language;
use codex_code_activity::SelectiveEditAnalysis;
use codex_code_activity::SelectiveEditDetails;
use codex_code_activity::SelectiveEditInput;
use codex_code_activity::SnapshotEpoch;
use codex_code_activity::SnapshotPreviewError;
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
use std::time::Duration;

pub fn analyze(source: &str) -> Result<SelectiveEditAnalysis, ActivityError> {
    let input = SelectiveEditInput::from_source(
        "authored-lazy-slices".into(),
        Language::Python,
        source,
        None,
        AnalysisLimits::new(32_768, Duration::from_secs(2)),
    )?;
    Ok(Analyzer::new()?.forecast_edits(input))
}

pub struct Fixture {
    pub analysis: SelectiveEditAnalysis,
    pub input: SnapshotPreviewInput,
}

impl Fixture {
    pub fn python(source: &str, read: Option<&[u8]>) -> Result<Self, ActivityError> {
        let analysis = analyze(source)?;
        let edit = &analysis.edits()[0];
        let SelectiveEditDetails::Python { prediction, .. } = &edit.details else {
            panic!("authored Python operation");
        };
        let before = TrustedTargetSnapshot::new(
            SnapshotTargetBinding::new(
                edit.evidence.source_id,
                SnapshotEpoch::new(prediction.ordinal),
                prediction.target.clone(),
            ),
            None,
        );
        let mut input = SnapshotPreviewInput::new(analysis.call_id().clone(), before);
        input.text_profile = SnapshotTextProfile::new(
            SnapshotReadNewline::PythonUniversal,
            SnapshotWriteNewline::Lf,
        );
        if let Some(bytes) = read {
            input.reads.push(TrustedReadSnapshot::new(
                SnapshotReadBinding::new(
                    SourceReadId::new(edit.evidence.source_id, 0),
                    SnapshotEpoch::new(0),
                    Target::Literal {
                        path: "input".into(),
                        cwd: None,
                    },
                    TextEncoding::Utf8,
                    SnapshotReadNewline::PythonUniversal,
                ),
                bytes.to_vec(),
            ));
        }
        Ok(Self { analysis, input })
    }

    pub fn preview(&self) -> Result<ConditionalSnapshotPreview, SnapshotPreviewError> {
        materialize_snapshot_preview(&self.analysis, 0, &self.input)
    }
}

pub fn transform(expression: &str, bytes: &[u8]) -> Result<Fixture, ActivityError> {
    Fixture::python(
        &format!(
            "from pathlib import Path\ns=Path('input').read_text(encoding='utf-8')\nPath('out').write_text({expression},encoding='utf-8')"
        ),
        Some(bytes),
    )
}
