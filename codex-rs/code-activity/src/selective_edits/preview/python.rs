//! Evaluate only the finite Python text/index/predicate recipe registry.

use super::SnapshotEpoch;
use super::SnapshotPreviewError;
use super::SnapshotReadNewline;
use super::SourceReadId;
use super::budget::Budget;
use super::snapshots::ReadExpectation;
use super::snapshots::Snapshots;
use super::text;
use crate::selective_edits::IndexRecipe;
use crate::selective_edits::PredicateRecipe;
use crate::selective_edits::PythonReadNewline;
use crate::selective_edits::TextRecipe;

pub(super) struct PythonEvaluator<'a, 'b> {
    pub(super) snapshots: &'b mut Snapshots<'a>,
    pub(super) budget: &'b mut Budget,
}

impl PythonEvaluator<'_, '_> {
    pub(super) fn text(
        &mut self,
        recipe: &TextRecipe,
        depth: usize,
    ) -> Result<String, SnapshotPreviewError> {
        self.budget.enter(depth)?;
        match recipe {
            TextRecipe::Literal { value, .. } => self.budget.copy(value),
            TextRecipe::Read {
                read_id,
                target,
                evidence,
                filesystem_epoch,
                encoding,
                newline,
            } => {
                let binding = ReadExpectation {
                    read: SourceReadId::new(evidence.source_id, *read_id),
                    filesystem_epoch: SnapshotEpoch::new(*filesystem_epoch),
                    target,
                    encoding: *encoding,
                    newline: match newline {
                        Some(PythonReadNewline::Preserve) => SnapshotReadNewline::Preserve,
                        None | Some(PythonReadNewline::UniversalTranslate) => {
                            SnapshotReadNewline::PythonUniversal
                        }
                    },
                };
                self.snapshots.read(binding, self.budget)
            }
            TextRecipe::Replace {
                input,
                old,
                new,
                count,
                ..
            } => {
                let input = self.text(input, depth + 1)?;
                text::replace(&input, old, new, *count, self.budget)
            }
            TextRecipe::ReplaceText {
                input,
                old,
                new,
                count,
                ..
            } => {
                let input = self.text(input, depth + 1)?;
                let old = self.text(old, depth + 1)?;
                let new = self.text(new, depth + 1)?;
                text::replace(&input, &old, &new, *count, self.budget)
            }
            TextRecipe::Slice {
                input, start, end, ..
            } => {
                let input = self.text(input, depth + 1)?;
                let start = start
                    .as_deref()
                    .map(|value| self.index(value, depth + 1))
                    .transpose()?;
                let end = end
                    .as_deref()
                    .map(|value| self.index(value, depth + 1))
                    .transpose()?;
                text::slice(&input, start, end, self.budget)
            }
            TextRecipe::SplitItem {
                input,
                separator,
                maxsplit,
                index,
                evidence,
                ..
            } => {
                let input = self.text(input, depth + 1)?;
                text::split_item(&input, separator, *maxsplit, *index, evidence, self.budget)
            }
            TextRecipe::Rstrip {
                input, characters, ..
            } => {
                let input = self.text(input, depth + 1)?;
                text::rstrip(&input, characters, self.budget)
            }
            TextRecipe::Concat { left, right, .. } => {
                let left = self.text(left, depth + 1)?;
                let right = self.text(right, depth + 1)?;
                text::concat(&left, &right, self.budget)
            }
            TextRecipe::Conditional {
                predicate,
                consequence,
                alternative,
                ..
            } => {
                let selected = if self.predicate(predicate, depth + 1)? {
                    consequence
                } else {
                    alternative
                };
                self.text(selected, depth + 1)
            }
        }
    }

    fn index(&mut self, recipe: &IndexRecipe, depth: usize) -> Result<i64, SnapshotPreviewError> {
        self.budget.enter(depth)?;
        match recipe {
            IndexRecipe::Literal { value, .. } => Ok(*value),
            IndexRecipe::SubstringIndex {
                input,
                needle,
                start,
                evidence,
            } => {
                let input = self.text(input, depth + 1)?;
                let start = start
                    .as_deref()
                    .map(|value| self.index(value, depth + 1))
                    .transpose()?;
                text::index(&input, needle, start, evidence, self.budget)
            }
        }
    }

    fn predicate(
        &mut self,
        recipe: &PredicateRecipe,
        depth: usize,
    ) -> Result<bool, SnapshotPreviewError> {
        self.budget.enter(depth)?;
        match recipe {
            PredicateRecipe::TextContains {
                needle, haystack, ..
            } => {
                let needle = self.text(needle, depth + 1)?;
                let haystack = self.text(haystack, depth + 1)?;
                self.budget
                    .work(needle.len().saturating_add(haystack.len()))?;
                Ok(haystack.contains(needle.as_str()))
            }
            PredicateRecipe::TextEquals { left, right, .. } => {
                let left = self.text(left, depth + 1)?;
                let right = self.text(right, depth + 1)?;
                self.budget.work(left.len().saturating_add(right.len()))?;
                Ok(left == right)
            }
            PredicateRecipe::And { left, right, .. } => {
                if !self.predicate(left, depth + 1)? {
                    return Ok(false);
                }
                self.predicate(right, depth + 1)
            }
            PredicateRecipe::StartsWith { input, prefix, .. } => {
                let input = self.text(input, depth + 1)?;
                self.budget.work(input.len().saturating_add(prefix.len()))?;
                Ok(input.starts_with(prefix))
            }
            PredicateRecipe::EndsWith { input, suffix, .. } => {
                let input = self.text(input, depth + 1)?;
                self.budget.work(input.len().saturating_add(suffix.len()))?;
                Ok(input.ends_with(suffix))
            }
        }
    }
}
