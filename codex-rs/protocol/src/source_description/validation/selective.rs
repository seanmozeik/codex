//! Structural/disclosure checks for conditional forecasts, not semantic proof.

mod carriers;
mod javascript;
mod python;
mod recipes;
mod snapshot;

use carriers::carrier_reason_valid;
use carriers::carriers_valid;
use carriers::outer_condition_valid;

use super::super::MAX_PATCH_BYTES;
use super::super::MAX_PATCHES;
use super::super::MAX_SOURCE_BYTES;
use super::super::MAX_SOURCES;
use super::super::SourceDescriptionLanguage;
use super::super::SourceDescriptionSource;
use super::super::SourceDescriptionSpan;
use super::super::SubmittedPatchFile;
use super::super::patch_validation::PatchBudget;
use super::super::selective::MAX_SELECTIVE_BARRIERS;
use super::super::selective::MAX_SELECTIVE_EDITS;
use super::super::selective::MAX_SELECTIVE_NODES;
use super::super::selective::MAX_SELECTIVE_RECIPE_DEPTH;
use super::super::selective::SelectiveBarrier;
use super::super::selective::SelectiveBarrierReason;
use super::super::selective::SelectiveCarrierAssumption;
use super::super::selective::SelectiveCarrierBasis;
use super::super::selective::SelectiveEditBasis;
use super::super::selective::SelectiveEditDetails;
use super::super::selective::SelectivePredictedEdit;
use super::super::selective::SelectivePythonAssumption;
use super::super::selective::SelectivePythonDependency;
use super::super::selective::SelectivePythonEdit;
use super::super::selective::SelectivePythonOperation;
use super::super::selective::SelectiveTarget;
use super::super::selective::SelectiveTextEncoding;
use super::display_text;
use std::collections::BTreeMap;

#[derive(Default)]
struct Budget {
    nodes: usize,
    text_bytes: usize,
}

impl Budget {
    fn node(&mut self) -> bool {
        self.nodes = self.nodes.saturating_add(1);
        self.nodes <= MAX_SELECTIVE_NODES
    }

    fn text(&mut self, value: &str, may_display: &impl Fn(&str) -> bool) -> bool {
        self.text_bytes = self.text_bytes.saturating_add(value.len());
        self.text_bytes <= MAX_SOURCE_BYTES && display_text(value, may_display)
    }

    fn target(&mut self, target: &SelectiveTarget, may_display: &impl Fn(&str) -> bool) -> bool {
        !target.path.is_empty()
            && self.text(&target.path, may_display)
            && target
                .cwd
                .as_ref()
                .is_none_or(|cwd| self.text(cwd, may_display))
    }

    fn span(&mut self, span: &SourceDescriptionSpan, sources: &[SourceDescriptionSource]) -> bool {
        self.node() && span.is_valid(sources)
    }
}

type ReadIdentity = (
    SourceDescriptionSpan,
    SelectiveTarget,
    usize,
    SelectiveTextEncoding,
    crate::source_description::SelectiveSnapshotReadNewline,
);

pub(super) fn is_presentable(
    sources: &[SourceDescriptionSource],
    edits: &[SelectivePredictedEdit],
    barriers: &[SelectiveBarrier],
    may_display: &impl Fn(&str) -> bool,
) -> bool {
    if sources.is_empty()
        || sources.len() > MAX_SOURCES
        || edits.len() > MAX_SELECTIVE_EDITS
        || barriers.len() > MAX_SELECTIVE_BARRIERS
    {
        return false;
    }
    let mut decoded_bytes = 0usize;
    for (id, source) in sources.iter().enumerate() {
        decoded_bytes = decoded_bytes.saturating_add(source.text.len());
        if source.id != id
            || decoded_bytes > MAX_SOURCE_BYTES
            || !display_text(&source.text, may_display)
        {
            return false;
        }
        match &source.parent {
            None if id == 0 => {}
            Some(parent)
                if parent.source_id < id
                    && if source.text.is_empty() {
                        parent.is_valid_or_empty(sources)
                    } else {
                        parent.is_valid(sources)
                    } => {}
            _ => return false,
        }
    }

    let mut budget = Budget::default();
    let mut patch_budget = PatchBudget::default();
    let mut patch_count = 0;
    let mut python_ordinals = BTreeMap::new();
    let mut javascript_ordinals = BTreeMap::new();
    let mut reads = BTreeMap::new();
    let mut snapshot_budget = snapshot::Budget::default();
    for (ordinal, edit) in edits.iter().enumerate() {
        if edit.ordinal != ordinal
            || !budget.span(&edit.evidence, sources)
            || !carriers_valid(
                sources,
                edit.evidence.source_id,
                &edit.carrier_assumptions,
                &mut budget,
            )
            || !edit.predecessor_conditions.iter().all(|condition| {
                budget.span(condition, sources) && outer_condition_valid(condition, edit, sources)
            })
        {
            return false;
        }
        match &edit.details {
            SelectiveEditDetails::Patch { patch, cwd } => {
                if edit.basis != SelectiveEditBasis::HostToolRequest
                    || edit.evidence != patch.invocation
                    || patch.id != patch_count
                    || patch_count >= MAX_PATCHES
                    || patch.payload.len() > MAX_PATCH_BYTES
                    || !display_text(&patch.payload, may_display)
                    || !budget.span(&patch.argument, sources)
                    || patch.argument.source_id != patch.invocation.source_id
                    || patch.argument.start_byte < patch.invocation.start_byte
                    || patch.argument.end_byte > patch.invocation.end_byte
                    || !patch.is_valid(&mut patch_budget)
                    || !patch.files.iter().all(|file| {
                        let (path, move_to) = match file {
                            SubmittedPatchFile::Add { path, .. }
                            | SubmittedPatchFile::Delete { path } => (path, None),
                            SubmittedPatchFile::Update { path, move_to, .. } => {
                                (path, move_to.as_ref())
                            }
                        };
                        budget.text(&path.spelling, may_display)
                            && move_to.is_none_or(|path| budget.text(&path.spelling, may_display))
                    })
                    || cwd
                        .as_ref()
                        .is_some_and(|cwd| !budget.text(cwd, may_display))
                {
                    return false;
                }
                decoded_bytes = decoded_bytes.saturating_add(patch.payload.len());
                if decoded_bytes > MAX_SOURCE_BYTES {
                    return false;
                }
                patch_count += 1;
            }
            SelectiveEditDetails::Python {
                prediction,
                assumptions,
            } => {
                let expected = python_ordinals
                    .entry(prediction.evidence.source_id)
                    .or_insert(0usize);
                if edit.basis != SelectiveEditBasis::StandardApiForecast
                    || edit.evidence != prediction.evidence
                    || prediction.ordinal != *expected
                    || edits[..ordinal].iter().any(|prior| {
                        prior.evidence.source_id == prediction.evidence.source_id
                            && matches!(&prior.details, SelectiveEditDetails::Python { prediction, .. }
                                if matches!(prediction.operation, SelectivePythonOperation::LogicalWrite(_)))
                    })
                    || !python::valid(
                        prediction,
                        assumptions,
                        sources,
                        &mut budget,
                        &mut reads,
                        may_display,
                    )
                {
                    return false;
                }
                *expected += 1;
            }
            SelectiveEditDetails::JavaScript {
                prediction,
                assumptions,
            } => {
                let expected = javascript_ordinals
                    .entry(prediction.evidence.source_id)
                    .or_insert(0usize);
                if edit.basis != SelectiveEditBasis::StandardApiForecast
                    || edit.evidence != prediction.evidence
                    || prediction.ordinal != *expected
                    || !javascript::valid(
                        prediction,
                        assumptions,
                        sources,
                        &mut budget,
                        &mut reads,
                        may_display,
                    )
                {
                    return false;
                }
                *expected += 1;
            }
        }
    }
    if !edits.iter().enumerate().all(|(ordinal, edit)| {
        snapshot::valid(
            edit,
            &edits[..ordinal],
            &reads,
            &mut snapshot_budget,
            may_display,
        )
    }) {
        return false;
    }
    barriers.iter().all(|barrier| {
        budget.span(&barrier.evidence, sources)
            && match &barrier.reason {
                SelectiveBarrierReason::Carrier { reason } => {
                    carrier_reason_valid(reason) && budget.text(reason, may_display)
                }
                SelectiveBarrierReason::Python { .. } => {
                    sources[barrier.evidence.source_id].language
                        == SourceDescriptionLanguage::Python
                }
                SelectiveBarrierReason::JavaScript { .. } => {
                    sources[barrier.evidence.source_id].language
                        == SourceDescriptionLanguage::TypeScript
                }
            }
    })
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod correction_tests;

#[cfg(test)]
mod recipe_tests;
