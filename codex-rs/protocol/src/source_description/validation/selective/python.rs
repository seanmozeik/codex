//! Python occurrence and dependency checks; they do not reconstruct semantic proof.

mod logical;

use super::Budget;
use super::MAX_SELECTIVE_EDITS;
use super::MAX_SELECTIVE_NODES;
use super::ReadIdentity;
use super::SelectivePythonAssumption;
use super::SelectivePythonDependency;
use super::SelectivePythonEdit;
use super::SelectivePythonOperation;
use super::SourceDescriptionLanguage;
use super::SourceDescriptionSource;
use super::SourceDescriptionSpan;
use super::recipes::RecipeValidation;
use std::collections::BTreeMap;

pub(super) fn valid(
    prediction: &SelectivePythonEdit,
    assumptions: &[SelectivePythonAssumption],
    sources: &[SourceDescriptionSource],
    budget: &mut Budget,
    reads: &mut BTreeMap<(usize, usize), ReadIdentity>,
    may_display: &impl Fn(&str) -> bool,
) -> bool {
    let source_id = prediction.evidence.source_id;
    if sources
        .get(source_id)
        .is_none_or(|source| source.language != SourceDescriptionLanguage::Python)
        || assumptions.len() > 6
        || !assumptions.contains(&SelectivePythonAssumption::StandardPythonEvaluation)
        || !assumptions.contains(&SelectivePythonAssumption::NoConcurrentMutation)
        || assumptions
            .iter()
            .enumerate()
            .any(|(i, value)| assumptions[..i].contains(value))
        || !budget.target(&prediction.target, may_display)
        || !iterations_valid(prediction, sources, budget)
        || !prediction.predecessor_conditions.iter().all(|condition| {
            condition.source_id == source_id
                && budget.span(condition, sources)
                && local_condition_valid(condition, prediction)
        })
        || !prediction.dependencies.iter().all(|dependency| {
            dependency_valid(
                dependency,
                source_id,
                sources,
                assumptions,
                budget,
                may_display,
            )
        })
        || !operation_dependencies_valid(prediction)
    {
        return false;
    }
    match &prediction.operation {
        SelectivePythonOperation::Truncate => true,
        SelectivePythonOperation::LogicalWrite(write) => {
            logical::valid(prediction, write, sources, budget)
                && RecipeValidation {
                    prediction,
                    sources,
                    budget,
                    reads,
                    may_display,
                }
                .text(&write.content, 0)
        }
        SelectivePythonOperation::WriteText { content, .. } => RecipeValidation {
            prediction,
            sources,
            budget,
            reads,
            may_display,
        }
        .text(content, 0),
    }
}

fn iterations_valid(
    prediction: &SelectivePythonEdit,
    sources: &[SourceDescriptionSource],
    budget: &mut Budget,
) -> bool {
    if prediction.iteration_context.len() > 4 {
        return false;
    }
    let mut enclosing: Option<&SourceDescriptionSpan> = None;
    prediction.iteration_context.iter().all(|iteration| {
        let evidence = &iteration.evidence;
        let valid = evidence.source_id == prediction.evidence.source_id
            && budget.span(evidence, sources)
            && (1..=64).contains(&iteration.literal_count)
            && iteration.index < iteration.literal_count
            && evidence.start_byte < prediction.evidence.start_byte
            && evidence.end_byte >= prediction.evidence.end_byte
            && enclosing.is_none_or(|outer| {
                outer.start_byte < evidence.start_byte && outer.end_byte >= evidence.end_byte
            });
        enclosing = Some(evidence);
        valid
    })
}

fn local_condition_valid(
    condition: &SourceDescriptionSpan,
    prediction: &SelectivePythonEdit,
) -> bool {
    let operation = &prediction.evidence;
    condition.end_byte <= operation.start_byte
        || (condition.start_byte >= operation.start_byte && condition.end_byte <= operation.end_byte && condition != operation)
        // A prior literal iteration can reuse a sink or a later byte range.
        // Only the recorded enclosing source loop can justify that ordering.
        || prediction.iteration_context.iter().any(|iteration| {
            iteration.index > 0
                && condition.start_byte > iteration.evidence.start_byte
                && condition.end_byte <= iteration.evidence.end_byte
        })
}

pub(super) fn has_capability(
    dependencies: &[SelectivePythonDependency],
    required: SelectivePythonAssumption,
) -> bool {
    dependencies.iter().any(|dependency| {
        matches!(dependency, SelectivePythonDependency::Capability { assumption } if *assumption == required)
    })
}

fn operation_dependencies_valid(prediction: &SelectivePythonEdit) -> bool {
    let dependencies = &prediction.dependencies;
    let common = has_capability(dependencies, SelectivePythonAssumption::StandardPythonEvaluation)
        && has_capability(dependencies, SelectivePythonAssumption::NoConcurrentMutation)
        && dependencies.iter().filter_map(|dependency| match dependency {
            SelectivePythonDependency::FilesystemEpoch { epoch } => Some(*epoch),
            _ => None,
        }).max() == Some(match &prediction.operation {
            SelectivePythonOperation::LogicalWrite(write) => write.base_filesystem_epoch,
            _ => prediction.ordinal,
        })
        && dependencies.iter().any(|dependency| {
            matches!(dependency, SelectivePythonDependency::EntryWorkingDirectory { cwd } if cwd == &prediction.target.cwd)
        })
        && dependencies.iter().all(|dependency| match dependency {
            SelectivePythonDependency::EntryWorkingDirectory { cwd } => cwd == &prediction.target.cwd,
            _ => true,
        });
    common
        && match &prediction.operation {
            SelectivePythonOperation::Truncate | SelectivePythonOperation::LogicalWrite(_) => {
                has_capability(dependencies, SelectivePythonAssumption::StandardBuiltinOpen)
            }
            SelectivePythonOperation::WriteText { .. } => {
                has_capability(dependencies, SelectivePythonAssumption::StandardBuiltinOpen)
                    || has_capability(dependencies, SelectivePythonAssumption::CanonicalPathlib)
            }
        }
}

fn dependency_valid(
    dependency: &SelectivePythonDependency,
    source_id: usize,
    sources: &[SourceDescriptionSource],
    assumptions: &[SelectivePythonAssumption],
    budget: &mut Budget,
    may_display: &impl Fn(&str) -> bool,
) -> bool {
    if !budget.node() {
        return false;
    }
    match dependency {
        SelectivePythonDependency::Binding {
            name,
            version,
            evidence,
        } => {
            !name.is_empty()
                && *version > 0
                && *version <= MAX_SELECTIVE_NODES
                && budget.text(name, may_display)
                && evidence.source_id == source_id
                && budget.span(evidence, sources)
        }
        SelectivePythonDependency::ReadValue { read_id, evidence } => {
            *read_id < MAX_SELECTIVE_NODES
                && evidence.source_id == source_id
                && budget.span(evidence, sources)
        }
        SelectivePythonDependency::EntryWorkingDirectory { cwd } => {
            cwd.as_ref().is_none_or(|cwd| budget.text(cwd, may_display))
        }
        SelectivePythonDependency::Capability { assumption } => assumptions.contains(assumption),
        SelectivePythonDependency::FilesystemEpoch { epoch } => *epoch <= MAX_SELECTIVE_EDITS,
    }
}
