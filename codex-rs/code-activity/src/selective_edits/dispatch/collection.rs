//! Ordered forecast accumulation and bounded result retention.

use super::Collection;
use super::MAX_BARRIERS;
use super::MAX_EDITS;
use super::MAX_RETAINED_BYTES;
use super::MAX_SOURCES;
use crate::Language;
use crate::PredictedEdit;
use crate::SelectiveBarrier;
use crate::SelectiveEditDetails;
use crate::SelectiveLaterContext;
use crate::Source;
use crate::SourceDescriptionFallback;
use crate::SourceId;
use crate::SourcePatchFile;
use crate::Span;
use crate::Target;
use crate::source_description::DescriptionBudget;

impl Collection {
    pub(super) fn source(
        &mut self,
        language: Language,
        source: &str,
        parent: Option<Span>,
    ) -> Result<SourceId, SourceDescriptionFallback> {
        self.charge_source(source.len())?;
        if self.result.sources.len() >= MAX_SOURCES {
            return Err(SourceDescriptionFallback::SourceLimit);
        }
        let id = SourceId(self.result.sources.len());
        self.result.sources.push(Source {
            id,
            language,
            parent,
        });
        self.result.source_texts.push(source.to_owned());
        Ok(id)
    }

    pub(super) fn charge_source(&mut self, bytes: usize) -> Result<(), SourceDescriptionFallback> {
        self.decoded_bytes = self
            .decoded_bytes
            .checked_add(bytes)
            .ok_or(SourceDescriptionFallback::SourceLimit)?;
        if self.decoded_bytes > self.source_limit {
            return Err(SourceDescriptionFallback::SourceLimit);
        }
        Ok(())
    }

    pub(super) fn push(
        &mut self,
        mut edit: PredictedEdit,
    ) -> Result<(), SourceDescriptionFallback> {
        if self.result.edits.len() >= MAX_EDITS {
            return Err(SourceDescriptionFallback::SourceLimit);
        }
        let next_targets = targets(&edit);
        if next_targets.is_empty() {
            return Err(SourceDescriptionFallback::UnsupportedSource);
        }
        for previous in &mut self.result.edits {
            if previous.later_context == SelectiveLaterContext::OpaqueSuffix {
                continue;
            }
            let same_spelling = targets(previous)
                .iter()
                .any(|target| next_targets.contains(target));
            if same_spelling {
                previous.later_context = SelectiveLaterContext::KnownLaterTouch;
            } else if previous.later_context == SelectiveLaterContext::Unobserved {
                previous.later_context = SelectiveLaterContext::PossibleAliasLaterTouch;
            }
        }
        edit.ordinal = self.result.edits.len();
        self.result.edits.push(edit);
        Ok(())
    }

    pub(super) fn barrier(
        &mut self,
        evidence: Span,
        reason: String,
    ) -> Result<(), SourceDescriptionFallback> {
        if self.result.barriers.len() >= MAX_BARRIERS {
            return Err(SourceDescriptionFallback::SourceLimit);
        }
        self.result
            .barriers
            .push(SelectiveBarrier { evidence, reason });
        self.opaque_suffix();
        Ok(())
    }

    pub(super) fn register_suffix_touches(
        &mut self,
        first_edit: usize,
        touches: &[super::python::PythonSuffixTouch],
    ) {
        for touch in touches {
            let Some(ordinal) = first_edit.checked_add(touch.edit_ordinal) else {
                continue;
            };
            let Some(edit) = self.result.edits.get(ordinal) else {
                continue;
            };
            let SelectiveEditDetails::Python { prediction, .. } = &edit.details else {
                continue;
            };
            let super::python::PythonOperation::LogicalWrite(write) = &prediction.operation else {
                continue;
            };
            if edit.evidence == touch.write_evidence
                && write.stream_id == touch.stream_id
                && write.opened_at == touch.opened_at
                && touch.flush_evidence.source_id == edit.evidence.source_id
                && touch.truncate_evidence.source_id == edit.evidence.source_id
                && edit.evidence.end_byte <= touch.flush_evidence.start_byte
                && touch.flush_evidence.end_byte <= touch.truncate_evidence.start_byte
            {
                self.suffix_touches.push((ordinal, edit.evidence.clone()));
            }
        }
    }

    pub(super) fn opaque_suffix(&mut self) {
        for edit in &mut self.result.edits {
            // Only a producer-proved target relation survives carrier unwind.
            // Python final-state opacity and the original barriers stay intact.
            edit.later_context =
                if self.suffix_touches.iter().any(|(ordinal, evidence)| {
                    *ordinal == edit.ordinal && *evidence == edit.evidence
                }) {
                    SelectiveLaterContext::KnownLaterTouch
                } else {
                    SelectiveLaterContext::OpaqueSuffix
                };
        }
    }

    pub(super) fn finish(
        &self,
        budget: DescriptionBudget,
    ) -> Result<(), SourceDescriptionFallback> {
        budget.check()?;
        let retained =
            serde_json::to_vec(&self.result).map_err(|_| SourceDescriptionFallback::SourceLimit)?;
        if retained.len().saturating_add(self.decoded_bytes) > MAX_RETAINED_BYTES {
            return Err(SourceDescriptionFallback::SourceLimit);
        }
        budget.check()
    }
}

fn targets(edit: &PredictedEdit) -> Vec<(&str, Option<&str>)> {
    match &edit.details {
        SelectiveEditDetails::Python { prediction, .. } => match &prediction.target {
            Target::Literal { path, cwd } => vec![(path.as_str(), cwd.as_deref())],
            Target::Unresolved => vec![],
        },
        SelectiveEditDetails::JavaScript { prediction, .. } => match &prediction.target {
            Target::Literal { path, cwd } => vec![(path.as_str(), cwd.as_deref())],
            Target::Unresolved => vec![],
        },
        SelectiveEditDetails::Patch { patch, cwd } => {
            let mut paths = Vec::new();
            for file in &patch.files {
                match file {
                    SourcePatchFile::Add { path, .. } | SourcePatchFile::Delete { path } => {
                        paths.push((path.spelling.as_str(), cwd.as_deref()));
                    }
                    SourcePatchFile::Update { path, move_to, .. } => {
                        paths.push((path.spelling.as_str(), cwd.as_deref()));
                        if let Some(path) = move_to {
                            paths.push((path.spelling.as_str(), cwd.as_deref()));
                        }
                    }
                }
            }
            paths
        }
    }
}
