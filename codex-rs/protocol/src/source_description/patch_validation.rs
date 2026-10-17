use super::SubmittedPatch;
use super::SubmittedPatchEvidence;
use super::SubmittedPatchFile;
use super::SubmittedPatchLine;
use super::SubmittedPatchLinePrefix;
use super::SubmittedPatchPath;

#[derive(Default)]
pub(super) struct PatchBudget {
    files: usize,
    chunks: usize,
    lines: usize,
}

impl SubmittedPatch {
    pub(super) fn is_valid(&self, budget: &mut PatchBudget) -> bool {
        budget.files = budget.files.saturating_add(self.files.len());
        budget.lines = budget.lines.saturating_add(self.payload.lines().count());
        if self.files.is_empty()
            || budget.files > 32
            || budget.lines > 4096
            || self.payload.contains(['\0', '\r', '\u{feff}'])
            || !self.payload.starts_with("*** Begin Patch\n")
        {
            return false;
        }
        let mut cursor = "*** Begin Patch\n".len();
        for file in &self.files {
            let valid = match file {
                SubmittedPatchFile::Add { path, lines } => {
                    self.path(path, "*** Add File: ", &mut cursor)
                        && !lines.is_empty()
                        && lines.iter().all(|line| {
                            line.prefix == SubmittedPatchLinePrefix::Plus
                                && self.line(line, &mut cursor)
                        })
                }
                SubmittedPatchFile::Delete { path } => {
                    self.path(path, "*** Delete File: ", &mut cursor)
                }
                SubmittedPatchFile::Update {
                    path,
                    move_to,
                    chunks,
                } => {
                    budget.chunks = budget.chunks.saturating_add(chunks.len());
                    self.path(path, "*** Update File: ", &mut cursor)
                        && move_to
                            .as_ref()
                            .is_none_or(|path| self.path(path, "*** Move to: ", &mut cursor))
                        && !chunks.is_empty()
                        && budget.chunks <= 128
                        && chunks.iter().enumerate().all(|(index, chunk)| {
                            let marker_valid = match &chunk.marker {
                                None => index == 0,
                                Some(marker) => self.evidence(marker).is_some_and(|text| {
                                    (text == "@@" || text.starts_with("@@ "))
                                        && self.consume(marker, &mut cursor)
                                }),
                            };
                            marker_valid
                                && !chunk.lines.is_empty()
                                && chunk.lines.iter().all(|line| self.line(line, &mut cursor))
                                && chunk.end_of_file.as_ref().is_none_or(|evidence| {
                                    index + 1 == chunks.len()
                                        && self.evidence(evidence) == Some("*** End of File")
                                        && self.consume(evidence, &mut cursor)
                                })
                        })
                }
            };
            if !valid {
                return false;
            }
        }
        matches!(
            self.payload.get(cursor..),
            Some("*** End Patch" | "*** End Patch\n")
        )
    }

    fn evidence<'a>(&'a self, evidence: &SubmittedPatchEvidence) -> Option<&'a str> {
        (evidence.patch_id == self.id)
            .then(|| self.payload.get(evidence.range.start..evidence.range.end))
            .flatten()
    }

    fn consume(&self, evidence: &SubmittedPatchEvidence, cursor: &mut usize) -> bool {
        let Some(text) = self.evidence(evidence) else {
            return false;
        };
        if evidence.range.start != *cursor
            || text.contains('\n')
            || self.payload.as_bytes().get(evidence.range.end) != Some(&b'\n')
        {
            return false;
        }
        *cursor = evidence.range.end + 1;
        true
    }

    fn path(&self, path: &SubmittedPatchPath, prefix: &str, cursor: &mut usize) -> bool {
        if path.spelling.is_empty()
            || path.spelling.starts_with(char::is_whitespace)
            || path.spelling.ends_with(char::is_whitespace)
            || path
                .spelling
                .chars()
                .any(|ch| ch.is_control() || matches!(ch, '\u{2028}' | '\u{2029}'))
            || !self
                .payload
                .get(*cursor..)
                .is_some_and(|rest| rest.starts_with(prefix))
            || self.evidence(&path.evidence) != Some(path.spelling.as_str())
        {
            return false;
        }
        *cursor += prefix.len();
        self.consume(&path.evidence, cursor)
    }

    fn line(&self, line: &SubmittedPatchLine, cursor: &mut usize) -> bool {
        let prefix = match line.prefix {
            SubmittedPatchLinePrefix::Context => b' ',
            SubmittedPatchLinePrefix::Plus => b'+',
            SubmittedPatchLinePrefix::Minus => b'-',
        };
        self.evidence(&line.evidence)
            .is_some_and(|text| text.as_bytes().first() == Some(&prefix))
            && line.text.range.start == line.evidence.range.start + 1
            && line.text.range.end == line.evidence.range.end
            && self.evidence(&line.text).is_some()
            && self.consume(&line.evidence, cursor)
    }
}
