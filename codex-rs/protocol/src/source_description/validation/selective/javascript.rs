//! Source-scoped Node provenance and bounded literal string replacements.

use super::Budget;
use super::MAX_SELECTIVE_EDITS;
use super::MAX_SELECTIVE_NODES;
use super::MAX_SELECTIVE_RECIPE_DEPTH;
use super::ReadIdentity;
use super::SelectiveTextEncoding;
use super::SourceDescriptionLanguage;
use super::SourceDescriptionSource;
use crate::source_description::SelectiveJavaScriptAssumption;
use crate::source_description::SelectiveJavaScriptDependency;
use crate::source_description::SelectiveJavaScriptEdit;
use crate::source_description::SelectiveJavaScriptTextRecipe;
use crate::source_description::SelectiveJavaScriptWriteEffect;
use std::collections::BTreeMap;

pub(super) fn valid(
    prediction: &SelectiveJavaScriptEdit,
    assumptions: &[SelectiveJavaScriptAssumption],
    sources: &[SourceDescriptionSource],
    budget: &mut Budget,
    reads: &mut BTreeMap<(usize, usize), ReadIdentity>,
    may_display: &impl Fn(&str) -> bool,
) -> bool {
    let source_id = prediction.evidence.source_id;
    if sources.get(source_id).is_none_or(|source| source.language != SourceDescriptionLanguage::TypeScript)
        || assumptions.len() > 6
        || assumptions.iter().enumerate().any(|(index, value)| assumptions[..index].contains(value))
        || ![
            SelectiveJavaScriptAssumption::StandardNodeCommonJsEvaluation,
            SelectiveJavaScriptAssumption::CanonicalNodeFs,
            SelectiveJavaScriptAssumption::NoConcurrentMutation,
        ].into_iter().all(|required| assumptions.contains(&required) && has_capability(prediction, required))
        || !budget.target(&prediction.target, may_display)
        || !effect_valid(prediction, assumptions, sources, budget)
        || !prediction.predecessor_conditions.iter().all(|condition| {
            condition.source_id == source_id && budget.span(condition, sources)
                && (condition.end_byte <= prediction.evidence.start_byte
                    || (condition.start_byte >= prediction.evidence.start_byte
                        && condition.end_byte <= prediction.evidence.end_byte
                        && condition != &prediction.evidence))
        })
        || !prediction.dependencies.iter().all(|dependency| {
            if !budget.node() { return false; }
            match dependency {
                SelectiveJavaScriptDependency::Descriptor { descriptor_id, opened_at } => {
                    *descriptor_id < MAX_SELECTIVE_NODES
                        && opened_at.source_id == source_id
                        && budget.span(opened_at, sources)
                        && (opened_at == &prediction.evidence || opened_at.end_byte <= prediction.evidence.start_byte)
                }
                SelectiveJavaScriptDependency::Binding { name, version, evidence } => {
                    !name.is_empty() && *version > 0 && *version <= MAX_SELECTIVE_NODES
                        && budget.text(name, may_display) && evidence.source_id == source_id && budget.span(evidence, sources)
                }
                SelectiveJavaScriptDependency::ReadValue { read_id, evidence } => {
                    *read_id < MAX_SELECTIVE_NODES && evidence.source_id == source_id && budget.span(evidence, sources)
                }
                SelectiveJavaScriptDependency::EntryWorkingDirectory { cwd } => {
                    cwd == &prediction.target.cwd && cwd.as_ref().is_none_or(|cwd| budget.text(cwd, may_display))
                }
                SelectiveJavaScriptDependency::FilesystemEpoch { epoch } => *epoch <= MAX_SELECTIVE_EDITS,
                SelectiveJavaScriptDependency::Capability { assumption } => assumptions.contains(assumption),
            }
        })
        || prediction.dependencies.iter().filter_map(|dependency| match dependency {
            SelectiveJavaScriptDependency::FilesystemEpoch { epoch } => Some(*epoch), _ => None,
        }).max() != Some(prediction.ordinal)
        || !prediction.dependencies.iter().any(|dependency| {
            matches!(dependency, SelectiveJavaScriptDependency::EntryWorkingDirectory { cwd } if cwd == &prediction.target.cwd)
        })
    {
        return false;
    }
    RecipeValidation {
        prediction,
        sources,
        budget,
        reads,
        may_display,
    }
    .text(&prediction.content, 0)
}

fn effect_valid(
    prediction: &SelectiveJavaScriptEdit,
    assumptions: &[SelectiveJavaScriptAssumption],
    sources: &[SourceDescriptionSource],
    budget: &mut Budget,
) -> bool {
    let (descriptor_id, opened_at) = match &prediction.write_effect {
        SelectiveJavaScriptWriteEffect::Overwrite => return true,
        SelectiveJavaScriptWriteEffect::Truncate {
            descriptor_id,
            opened_at,
        } => {
            if opened_at != &prediction.evidence
                || !matches!(&prediction.content, SelectiveJavaScriptTextRecipe::Literal { value, .. } if value.is_empty())
            {
                return false;
            }
            (descriptor_id, opened_at)
        }
        SelectiveJavaScriptWriteEffect::DescriptorAppend {
            descriptor_id,
            opened_at,
        } => {
            if opened_at.end_byte > prediction.evidence.start_byte
                || !assumptions.contains(&SelectiveJavaScriptAssumption::CompleteDescriptorWrites)
                || !has_capability(
                    prediction,
                    SelectiveJavaScriptAssumption::CompleteDescriptorWrites,
                )
                || !prediction.predecessor_conditions.contains(opened_at)
            {
                return false;
            }
            (descriptor_id, opened_at)
        }
        SelectiveJavaScriptWriteEffect::DescriptorWrite {
            descriptor_id,
            opened_at,
            byte_offset,
            ..
        } => {
            if !u64::try_from(*byte_offset).is_ok_and(|value| value <= 9_007_199_254_740_991)
                || opened_at.end_byte > prediction.evidence.start_byte
                || !assumptions.contains(&SelectiveJavaScriptAssumption::CompleteDescriptorWrites)
                || !has_capability(
                    prediction,
                    SelectiveJavaScriptAssumption::CompleteDescriptorWrites,
                )
                || !prediction.predecessor_conditions.contains(opened_at)
            {
                return false;
            }
            (descriptor_id, opened_at)
        }
    };
    *descriptor_id < MAX_SELECTIVE_NODES
        && opened_at.source_id == prediction.evidence.source_id
        && budget.span(opened_at, sources)
        && assumptions.contains(&SelectiveJavaScriptAssumption::RegularDescriptorFiles)
        && has_capability(prediction, SelectiveJavaScriptAssumption::RegularDescriptorFiles)
        && prediction.dependencies.iter().any(|dependency| {
            matches!(dependency, SelectiveJavaScriptDependency::Descriptor { descriptor_id: id, opened_at: opening }
                if id == descriptor_id && opening == opened_at)
        })
}

fn has_capability(
    prediction: &SelectiveJavaScriptEdit,
    required: SelectiveJavaScriptAssumption,
) -> bool {
    prediction.dependencies.iter().any(|dependency| {
        matches!(dependency, SelectiveJavaScriptDependency::Capability { assumption } if *assumption == required)
    })
}

struct RecipeValidation<'a, F> {
    prediction: &'a SelectiveJavaScriptEdit,
    sources: &'a [SourceDescriptionSource],
    budget: &'a mut Budget,
    reads: &'a mut BTreeMap<(usize, usize), ReadIdentity>,
    may_display: &'a F,
}

impl<F: Fn(&str) -> bool> RecipeValidation<'_, F> {
    fn text(&mut self, recipe: &SelectiveJavaScriptTextRecipe, depth: usize) -> bool {
        let evidence = match recipe {
            SelectiveJavaScriptTextRecipe::Literal { evidence, .. }
            | SelectiveJavaScriptTextRecipe::ReadUtf8 { evidence, .. }
            | SelectiveJavaScriptTextRecipe::ReplaceFirst { evidence, .. }
            | SelectiveJavaScriptTextRecipe::ReplaceAll { evidence, .. }
            | SelectiveJavaScriptTextRecipe::Slice { evidence, .. }
            | SelectiveJavaScriptTextRecipe::Concat { evidence, .. } => evidence,
        };
        if depth > MAX_SELECTIVE_RECIPE_DEPTH
            || !self.budget.node()
            || evidence.source_id != self.prediction.evidence.source_id
            || !self.budget.span(evidence, self.sources)
        {
            return false;
        }
        match recipe {
            SelectiveJavaScriptTextRecipe::Literal { value, .. } => {
                self.budget.text(value, self.may_display)
            }
            SelectiveJavaScriptTextRecipe::ReadUtf8 {
                read_id,
                target,
                filesystem_epoch,
                ..
            } => {
                if *read_id >= MAX_SELECTIVE_NODES || *filesystem_epoch > self.prediction.ordinal
                    || !self.budget.target(target, self.may_display)
                    || !self.prediction.dependencies.iter().any(|dependency| {
                        matches!(dependency, SelectiveJavaScriptDependency::ReadValue { read_id: id, evidence: origin } if read_id == id && evidence == origin)
                    })
                    || !self.prediction.dependencies.iter().any(|dependency| {
                        matches!(dependency, SelectiveJavaScriptDependency::FilesystemEpoch { epoch } if epoch == filesystem_epoch)
                    })
                    || !self.prediction.dependencies.iter().any(|dependency| {
                        matches!(dependency, SelectiveJavaScriptDependency::EntryWorkingDirectory { cwd } if cwd == &target.cwd)
                    })
                {
                    return false;
                }
                let identity = (
                    evidence.clone(),
                    target.clone(),
                    *filesystem_epoch,
                    SelectiveTextEncoding::Utf8,
                    crate::source_description::SelectiveSnapshotReadNewline::Preserve,
                );
                let key = (evidence.source_id, *read_id);
                match self.reads.get(&key) {
                    Some(existing) => existing == &identity,
                    None => {
                        self.reads.insert(key, identity);
                        true
                    }
                }
            }
            SelectiveJavaScriptTextRecipe::ReplaceFirst {
                input, old, new, ..
            } => {
                has_capability(
                    self.prediction,
                    SelectiveJavaScriptAssumption::StandardStringMethods,
                ) && !new.contains('$')
                    && self.budget.text(old, self.may_display)
                    && self.budget.text(new, self.may_display)
                    && self.text(input, depth + 1)
            }
            SelectiveJavaScriptTextRecipe::ReplaceAll {
                input, old, new, ..
            } => {
                !old.is_empty()
                    && has_capability(
                        self.prediction,
                        SelectiveJavaScriptAssumption::StandardStringMethods,
                    )
                    && !new.contains('$')
                    && self.budget.text(old, self.may_display)
                    && self.budget.text(new, self.may_display)
                    && self.text(input, depth + 1)
            }
            SelectiveJavaScriptTextRecipe::Slice {
                input, start, end, ..
            } => {
                let safe = |value: usize| {
                    u64::try_from(value).is_ok_and(|value| value <= 9_007_199_254_740_991)
                };
                safe(*start)
                    && end.is_none_or(safe)
                    && has_capability(
                        self.prediction,
                        SelectiveJavaScriptAssumption::StandardStringMethods,
                    )
                    && self.text(input, depth + 1)
            }
            SelectiveJavaScriptTextRecipe::Concat { left, right, .. } => {
                has_capability(
                    self.prediction,
                    SelectiveJavaScriptAssumption::StandardStringMethods,
                ) && self.text(left, depth + 1)
                    && self.text(right, depth + 1)
            }
        }
    }
}

#[cfg(test)]
#[path = "javascript_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "javascript_append_tests.rs"]
mod append_tests;
