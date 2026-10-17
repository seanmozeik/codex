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

pub struct Fixture {
    pub analysis: SelectiveEditAnalysis,
    pub input: SnapshotPreviewInput,
    pub ordinal: usize,
}

impl Fixture {
    pub fn python(
        source: &str,
        ordinal: usize,
        before: Option<&[u8]>,
    ) -> Result<Self, ActivityError> {
        let input = SelectiveEditInput::from_source(
            "authored-snapshot-preview".into(),
            Language::Python,
            source,
            Some("/authored"),
            AnalysisLimits::new(32_768, Duration::from_secs(2)),
        )?;
        Self::new(
            input,
            ordinal,
            before,
            SnapshotTextProfile::new(
                SnapshotReadNewline::PythonUniversal,
                SnapshotWriteNewline::Lf,
            ),
        )
    }

    pub fn node(source: &str, before: Option<&[u8]>) -> Result<Self, ActivityError> {
        Self::node_at(source, 0, before)
    }

    pub fn node_at(
        source: &str,
        ordinal: usize,
        before: Option<&[u8]>,
    ) -> Result<Self, ActivityError> {
        let input = SelectiveEditInput::from_node_commonjs(
            "authored-snapshot-preview".into(),
            source,
            Some("/authored"),
            AnalysisLimits::new(32_768, Duration::from_secs(2)),
        )?;
        Self::new(
            input,
            ordinal,
            before,
            SnapshotTextProfile::new(
                SnapshotReadNewline::Preserve,
                SnapshotWriteNewline::Preserve,
            ),
        )
    }

    pub fn shell(
        source: &str,
        ordinal: usize,
        before: Option<&[u8]>,
    ) -> Result<Self, ActivityError> {
        let input = SelectiveEditInput::from_source(
            "authored-snapshot-preview".into(),
            Language::Shell,
            source,
            Some("/authored"),
            AnalysisLimits::new(32_768, Duration::from_secs(2)),
        )?;
        Self::new(
            input,
            ordinal,
            before,
            SnapshotTextProfile::new(
                SnapshotReadNewline::PythonUniversal,
                SnapshotWriteNewline::Lf,
            ),
        )
    }

    fn new(
        input: SelectiveEditInput,
        ordinal: usize,
        before: Option<&[u8]>,
        profile: SnapshotTextProfile,
    ) -> Result<Self, ActivityError> {
        let analysis = Analyzer::new()?.forecast_edits(input);
        let edit = &analysis.edits()[ordinal];
        let (target, local) = match &edit.details {
            SelectiveEditDetails::Python { prediction, .. } => {
                (prediction.target.clone(), prediction.ordinal)
            }
            SelectiveEditDetails::JavaScript { prediction, .. } => {
                (prediction.target.clone(), prediction.ordinal)
            }
            _ => panic!("authored text operation"),
        };
        let before = TrustedTargetSnapshot::new(
            SnapshotTargetBinding::new(edit.evidence.source_id, SnapshotEpoch::new(local), target),
            before.map(<[u8]>::to_vec),
        );
        let mut input = SnapshotPreviewInput::new(analysis.call_id().clone(), before);
        input.text_profile = profile;
        Ok(Self {
            analysis,
            input,
            ordinal,
        })
    }

    pub fn read(
        &mut self,
        read_id: usize,
        epoch: usize,
        path: &str,
        bytes: &[u8],
        encoding: TextEncoding,
    ) {
        self.input.reads.push(TrustedReadSnapshot::new(
            SnapshotReadBinding::new(
                SourceReadId::new(
                    self.analysis.edits()[self.ordinal].evidence.source_id,
                    read_id,
                ),
                SnapshotEpoch::new(epoch),
                Target::Literal {
                    path: path.into(),
                    cwd: Some("/authored".into()),
                },
                encoding,
                self.input.text_profile.read_newline,
            ),
            bytes.to_vec(),
        ));
    }

    pub fn preview(&self) -> Result<ConditionalSnapshotPreview, SnapshotPreviewError> {
        materialize_snapshot_preview(&self.analysis, self.ordinal, &self.input)
    }
}
