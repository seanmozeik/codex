//! One recursive budget covers Python text, integer and predicate nodes.

use super::Budget;
use super::MAX_SELECTIVE_NODES;
use super::MAX_SELECTIVE_RECIPE_DEPTH;
use super::ReadIdentity;
use super::SelectivePythonAssumption;
use super::SelectivePythonDependency;
use super::SelectivePythonEdit;
use super::SourceDescriptionSource;
use super::SourceDescriptionSpan;
use super::python::has_capability;
use crate::source_description::SelectiveIndexRecipe;
use crate::source_description::SelectivePredicateRecipe;
use crate::source_description::SelectiveTextRecipe;
use std::collections::BTreeMap;

pub(super) struct RecipeValidation<'a, F> {
    pub(super) prediction: &'a SelectivePythonEdit,
    pub(super) sources: &'a [SourceDescriptionSource],
    pub(super) budget: &'a mut Budget,
    pub(super) reads: &'a mut BTreeMap<(usize, usize), ReadIdentity>,
    pub(super) may_display: &'a F,
}

impl<F: Fn(&str) -> bool> RecipeValidation<'_, F> {
    pub(super) fn text(&mut self, recipe: &SelectiveTextRecipe, depth: usize) -> bool {
        let evidence = match recipe {
            SelectiveTextRecipe::Literal { evidence, .. }
            | SelectiveTextRecipe::Read { evidence, .. }
            | SelectiveTextRecipe::Replace { evidence, .. }
            | SelectiveTextRecipe::ReplaceText { evidence, .. }
            | SelectiveTextRecipe::Slice { evidence, .. }
            | SelectiveTextRecipe::SplitItem { evidence, .. }
            | SelectiveTextRecipe::Rstrip { evidence, .. }
            | SelectiveTextRecipe::Concat { evidence, .. }
            | SelectiveTextRecipe::Conditional { evidence, .. } => evidence,
        };
        if !self.node(evidence, depth) {
            return false;
        }
        match recipe {
            SelectiveTextRecipe::Literal { value, .. } => self.budget.text(value, self.may_display),
            SelectiveTextRecipe::Read {
                read_id,
                target,
                filesystem_epoch,
                encoding,
                newline,
                ..
            } => {
                if *read_id >= MAX_SELECTIVE_NODES || *filesystem_epoch > match &self.prediction.operation {
                    crate::source_description::SelectivePythonOperation::LogicalWrite(write) => write.base_filesystem_epoch,
                    _ => self.prediction.ordinal,
                }
                    || !self.budget.target(target, self.may_display)
                    || !self.prediction.dependencies.iter().any(|dependency| {
                        matches!(dependency, SelectivePythonDependency::ReadValue { read_id: id, evidence: origin } if read_id == id && evidence == origin)
                    })
                    || !self.prediction.dependencies.iter().any(|dependency| {
                        matches!(dependency, SelectivePythonDependency::FilesystemEpoch { epoch } if epoch == filesystem_epoch)
                    })
                    || !self.prediction.dependencies.iter().any(|dependency| {
                        matches!(dependency, SelectivePythonDependency::EntryWorkingDirectory { cwd } if cwd == &target.cwd)
                    })
                {
                    return false;
                }
                let identity = (
                    evidence.clone(),
                    target.clone(),
                    *filesystem_epoch,
                    *encoding,
                    match newline {
                        Some(crate::source_description::SelectivePythonReadNewline::Preserve) => {
                            crate::source_description::SelectiveSnapshotReadNewline::Preserve
                        }
                        _ => {
                            crate::source_description::SelectiveSnapshotReadNewline::PythonUniversal
                        }
                    },
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
            SelectiveTextRecipe::Replace {
                input,
                old,
                new,
                count,
                ..
            } => {
                self.methods()
                    && count.is_none_or(safe_count)
                    && self.budget.text(old, self.may_display)
                    && self.budget.text(new, self.may_display)
                    && self.text(input, depth + 1)
            }
            SelectiveTextRecipe::ReplaceText {
                input,
                old,
                new,
                count,
                ..
            } => {
                self.methods()
                    && count.is_none_or(safe_count)
                    && self.text(input, depth + 1)
                    && self.text(old, depth + 1)
                    && self.text(new, depth + 1)
            }
            SelectiveTextRecipe::Slice {
                input, start, end, ..
            } => {
                self.methods()
                    && self.text(input, depth + 1)
                    && start
                        .as_ref()
                        .is_none_or(|value| self.index(value, depth + 1))
                    && end
                        .as_ref()
                        .is_none_or(|value| self.index(value, depth + 1))
            }
            SelectiveTextRecipe::SplitItem {
                input,
                separator,
                maxsplit,
                index,
                split_evidence,
                ..
            } => {
                self.methods()
                    && !separator.is_empty()
                    && safe_integer(*index)
                    && maxsplit.is_none_or(safe_integer)
                    && self.budget.text(separator, self.may_display)
                    && split_evidence.source_id == evidence.source_id
                    && (split_evidence.end_byte <= evidence.start_byte
                        || (split_evidence.start_byte >= evidence.start_byte
                            && split_evidence.end_byte <= evidence.end_byte)
                        || self.prediction.iteration_context.iter().any(|iteration| {
                            iteration.index > 0
                                && split_evidence.start_byte > iteration.evidence.start_byte
                                && split_evidence.end_byte <= iteration.evidence.end_byte
                        }))
                    && self.budget.span(split_evidence, self.sources)
                    && self.text(input, depth + 1)
            }
            SelectiveTextRecipe::Rstrip {
                input, characters, ..
            } => {
                self.methods()
                    && self.budget.text(characters, self.may_display)
                    && self.text(input, depth + 1)
            }
            SelectiveTextRecipe::Concat { left, right, .. } => {
                self.methods() && self.text(left, depth + 1) && self.text(right, depth + 1)
            }
            SelectiveTextRecipe::Conditional {
                predicate,
                consequence,
                alternative,
                ..
            } => {
                self.methods()
                    && self.predicate(predicate, depth + 1)
                    && self.text(consequence, depth + 1)
                    && self.text(alternative, depth + 1)
            }
        }
    }

    fn index(&mut self, recipe: &SelectiveIndexRecipe, depth: usize) -> bool {
        match recipe {
            SelectiveIndexRecipe::Literal { value, evidence } => {
                self.node(evidence, depth) && safe_integer(*value)
            }
            SelectiveIndexRecipe::SubstringIndex {
                input,
                needle,
                start,
                evidence,
            } => {
                self.node(evidence, depth)
                    && self.methods()
                    && self.budget.text(needle, self.may_display)
                    && self.text(input, depth + 1)
                    && start
                        .as_ref()
                        .is_none_or(|value| self.index(value, depth + 1))
            }
        }
    }

    fn predicate(&mut self, recipe: &SelectivePredicateRecipe, depth: usize) -> bool {
        let evidence = match recipe {
            SelectivePredicateRecipe::TextContains { evidence, .. }
            | SelectivePredicateRecipe::TextEquals { evidence, .. }
            | SelectivePredicateRecipe::And { evidence, .. }
            | SelectivePredicateRecipe::StartsWith { evidence, .. }
            | SelectivePredicateRecipe::EndsWith { evidence, .. } => evidence,
        };
        if !self.node(evidence, depth) || !self.methods() {
            return false;
        }
        match recipe {
            SelectivePredicateRecipe::TextContains {
                needle, haystack, ..
            } => self.text(needle, depth + 1) && self.text(haystack, depth + 1),
            SelectivePredicateRecipe::TextEquals { left, right, .. } => {
                self.text(left, depth + 1) && self.text(right, depth + 1)
            }
            SelectivePredicateRecipe::And { left, right, .. } => {
                // Disclosure/provenance validation covers both closed sides,
                // regardless of which side materialization would evaluate.
                self.predicate(left, depth + 1) && self.predicate(right, depth + 1)
            }
            SelectivePredicateRecipe::StartsWith {
                input,
                prefix: value,
                prefix_evidence: value_evidence,
                ..
            }
            | SelectivePredicateRecipe::EndsWith {
                input,
                suffix: value,
                suffix_evidence: value_evidence,
                ..
            } => {
                value_evidence.source_id == evidence.source_id
                    && value_evidence.start_byte >= evidence.start_byte
                    && value_evidence.end_byte <= evidence.end_byte
                    && self.budget.span(value_evidence, self.sources)
                    && self.budget.text(value, self.may_display)
                    && self.text(input, depth + 1)
            }
        }
    }

    fn node(&mut self, evidence: &SourceDescriptionSpan, depth: usize) -> bool {
        depth <= MAX_SELECTIVE_RECIPE_DEPTH
            && self.budget.node()
            && evidence.source_id == self.prediction.evidence.source_id
            && self.budget.span(evidence, self.sources)
    }

    fn methods(&self) -> bool {
        has_capability(
            &self.prediction.dependencies,
            SelectivePythonAssumption::StandardTextMethods,
        )
    }
}

// Wire numbers must remain exact in JavaScript clients. Larger Python integers
// retain their native semantics but cannot be published through this DTO.
fn safe_integer(value: i64) -> bool {
    (-9_007_199_254_740_991..=9_007_199_254_740_991).contains(&value)
}

fn safe_count(value: usize) -> bool {
    (value as u128) <= 9_007_199_254_740_991
}
