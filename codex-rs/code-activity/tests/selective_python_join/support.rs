use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::ConditionalSnapshotPreview;
use codex_code_activity::Language;
use codex_code_activity::SelectiveEditAnalysis;
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
use codex_code_activity::Span;
use codex_code_activity::Target;
use codex_code_activity::TextEncoding;
use codex_code_activity::TrustedReadSnapshot;
use codex_code_activity::TrustedTargetSnapshot;
use codex_code_activity::materialize_snapshot_preview;
use serde_json::Value;
use std::time::Duration;

pub type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub fn cases() -> TestResult<Vec<Value>> {
    let document: Value = serde_json::from_str(include_str!("fixtures.json"))?;
    Ok(document["cases"].as_array().ok_or("fixture cases")?.clone())
}

pub fn case(id: &str) -> TestResult<Value> {
    cases()?
        .into_iter()
        .find(|row| row["id"] == id)
        .ok_or_else(|| "fixture id".into())
}

pub fn text<'a>(row: &'a Value, key: &str) -> TestResult<&'a str> {
    Ok(row[key].as_str().ok_or("fixture string field")?)
}

pub fn analyze(source: &str) -> TestResult<SelectiveEditAnalysis> {
    let input = SelectiveEditInput::from_source(
        "authored-join-development".into(),
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
    pub fn new(row: &Value) -> TestResult<Self> {
        let analysis = analyze(text(row, "source")?)?;
        let source_id = analysis.sources().first().ok_or("source table")?.id;
        // Independently authored target/epoch, not copied from the prediction.
        let target = Target::Literal {
            path: "result.txt".into(),
            cwd: None,
        };
        let before = TrustedTargetSnapshot::new(
            SnapshotTargetBinding::new(source_id, SnapshotEpoch::new(0), target),
            Some(b"old".to_vec()),
        );
        let mut input = SnapshotPreviewInput::new(analysis.call_id().clone(), before);
        input.text_profile = SnapshotTextProfile::new(
            SnapshotReadNewline::PythonUniversal,
            SnapshotWriteNewline::Lf,
        );
        if let Some(bytes) = row["read"].as_str() {
            input.reads.push(TrustedReadSnapshot::new(
                SnapshotReadBinding::new(
                    SourceReadId::new(source_id, 0),
                    SnapshotEpoch::new(0),
                    Target::Literal {
                        path: "input".into(),
                        cwd: None,
                    },
                    TextEncoding::Utf8,
                    SnapshotReadNewline::PythonUniversal,
                ),
                bytes.as_bytes().to_vec(),
            ));
        }
        Ok(Self { analysis, input })
    }

    pub fn preview(&self) -> Result<ConditionalSnapshotPreview, SnapshotPreviewError> {
        materialize_snapshot_preview(&self.analysis, 0, &self.input)
    }
}

pub fn span_text<'a>(source: &'a str, span: &Span) -> TestResult<&'a str> {
    Ok(source
        .get(span.start_byte..span.end_byte)
        .ok_or("source span")?)
}
