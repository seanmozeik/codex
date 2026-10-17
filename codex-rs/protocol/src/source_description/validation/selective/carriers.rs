//! Ordered carrier premises and source-scoped predecessor evidence.

use super::Budget;
use super::SelectiveCarrierAssumption;
use super::SelectiveCarrierBasis;
use super::SelectivePredictedEdit;
use super::SourceDescriptionLanguage;
use super::SourceDescriptionSource;
use super::SourceDescriptionSpan;

// Each owned child requires an ordered dispatch hypothesis. Python adds a
// second, adjacent version hypothesis with exactly the same invocation evidence.
// The source table is already validated, so parent IDs strictly decrease.
pub(super) fn carriers_valid(
    sources: &[SourceDescriptionSource],
    mut source_id: usize,
    carriers: &[SelectiveCarrierAssumption],
    budget: &mut Budget,
) -> bool {
    let mut chain = Vec::new();
    while let Some(parent) = sources[source_id].parent.as_ref() {
        chain.push(source_id);
        source_id = parent.source_id;
    }
    let mut assumptions = carriers.iter();
    for child_id in chain.into_iter().rev() {
        let child = &sources[child_id];
        let Some(parent) = &child.parent else {
            return false;
        };
        if !matches!(
            (sources[parent.source_id].language, child.language),
            (
                SourceDescriptionLanguage::TypeScript,
                SourceDescriptionLanguage::Shell
            ) | (
                SourceDescriptionLanguage::Shell,
                SourceDescriptionLanguage::Shell
                    | SourceDescriptionLanguage::Python
                    | SourceDescriptionLanguage::TypeScript
            )
        ) {
            return false;
        }
        let Some(dispatch) = assumptions.next() else {
            return false;
        };
        if dispatch.basis != SelectiveCarrierBasis::StandardShellDispatchAndStartup
            || dispatch.language != child.language
            || !budget.span(&dispatch.invocation, sources)
            || dispatch.invocation.source_id != parent.source_id
            || dispatch.invocation.start_byte > parent.start_byte
            || dispatch.invocation.end_byte < parent.end_byte
        {
            return false;
        }
        let version_basis = match child.language {
            SourceDescriptionLanguage::Python => Some(SelectiveCarrierBasis::StandardPython3Cli),
            SourceDescriptionLanguage::TypeScript => {
                Some(SelectiveCarrierBasis::StandardNodeCommonJsCli)
            }
            SourceDescriptionLanguage::Shell => None,
        };
        if let Some(version_basis) = version_basis {
            let Some(version) = assumptions.next() else {
                return false;
            };
            if version.basis != version_basis
                || version.language != child.language
                || version.invocation != dispatch.invocation
                || !budget.span(&version.invocation, sources)
            {
                return false;
            }
        }
    }
    assumptions.next().is_none()
}

pub(super) fn outer_condition_valid(
    condition: &SourceDescriptionSpan,
    edit: &SelectivePredictedEdit,
    sources: &[SourceDescriptionSource],
) -> bool {
    if condition.source_id == edit.evidence.source_id {
        return condition.end_byte <= edit.evidence.start_byte;
    }
    let mut child_id = edit.evidence.source_id;
    while let Some(parent) = sources[child_id].parent.as_ref() {
        if condition.source_id == parent.source_id {
            return edit.carrier_assumptions.iter().any(|carrier| {
                carrier.basis == SelectiveCarrierBasis::StandardShellDispatchAndStartup
                    && carrier.invocation.source_id == parent.source_id
                    && condition.end_byte <= carrier.invocation.start_byte
            });
        }
        child_id = parent.source_id;
    }
    false
}

pub(super) fn carrier_reason_valid(reason: &str) -> bool {
    matches!(
        reason,
        "unknownExecution"
            | "unsupportedControl"
            | "unresolvedDispatch"
            | "unresolvedArguments"
            | "contextMutation"
            | "concurrentExecution"
            | "unresolvedPatch"
            | "unresolvedChild"
    )
}
