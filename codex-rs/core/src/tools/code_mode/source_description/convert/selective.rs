//! Explicit conversion of conditional forecasts; no legacy partial-report escape.

mod javascript;
mod python;
mod recipes;

use super::Unsupported;
use super::activity;
use super::file;
use super::source_description_for_display;
use super::span;
use super::wire;
use python::assumption;
use python::python;

pub(super) fn description(result: &activity::SelectiveEditAnalysis) -> wire::SourceDescription {
    let reason = match result.disposition() {
        activity::SelectiveEditDisposition::Analyzed => None,
        activity::SelectiveEditDisposition::Unavailable(reason) => Some(match reason {
            activity::SourceDescriptionFallback::SourceLimit => {
                wire::SourceDescriptionFallback::SourceLimit
            }
            activity::SourceDescriptionFallback::TimeBudget => {
                wire::SourceDescriptionFallback::Deadline
            }
            _ => wire::SourceDescriptionFallback::UnsupportedSource,
        }),
    };
    if let Some(reason) = reason {
        return wire::SourceDescription::Fallback { reason };
    }
    source_description_for_display(
        convert(result).unwrap_or(wire::SourceDescription::Fallback {
            reason: wire::SourceDescriptionFallback::UnsupportedSource,
        }),
    )
}

fn convert(
    result: &activity::SelectiveEditAnalysis,
) -> Result<wire::SourceDescription, Unsupported> {
    if result.sources().len() != result.source_texts().len() {
        return Err(Unsupported);
    }
    let sources = result
        .sources()
        .iter()
        .zip(result.source_texts())
        .map(|(source, text)| {
            Ok(wire::SourceDescriptionSource {
                id: source.id.index(),
                language: language(source.language)?,
                parent: source.parent.as_ref().map(span),
                text: text.clone(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let edits = result
        .edits()
        .iter()
        .map(edit)
        .collect::<Result<Vec<_>, _>>()?;
    let barriers = result
        .barriers()
        .iter()
        .map(|barrier| {
            Ok(wire::SelectiveBarrier {
                evidence: span(&barrier.evidence),
                reason: wire::SelectiveBarrierReason::Carrier {
                    reason: barrier.reason.clone(),
                },
            })
        })
        .chain(result.python_barriers().iter().map(|barrier| {
            Ok(wire::SelectiveBarrier {
                evidence: span(&barrier.evidence),
                reason: wire::SelectiveBarrierReason::Python {
                    reason: match barrier.reason {
                        activity::PythonBarrierReason::UnknownName => {
                            wire::SelectivePythonBarrierReason::UnknownName
                        }
                        activity::PythonBarrierReason::UnmodeledCall => {
                            wire::SelectivePythonBarrierReason::UnmodeledCall
                        }
                        activity::PythonBarrierReason::UnmodeledImport => {
                            wire::SelectivePythonBarrierReason::UnmodeledImport
                        }
                        activity::PythonBarrierReason::UnsupportedOperation => {
                            wire::SelectivePythonBarrierReason::UnsupportedOperation
                        }
                        activity::PythonBarrierReason::EscapingHandle => {
                            wire::SelectivePythonBarrierReason::EscapingHandle
                        }
                    },
                },
            })
        }))
        .chain(result.javascript_barriers().iter().map(javascript::barrier))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(wire::SourceDescription::SelectiveFindings {
        sources,
        edits,
        barriers,
    })
}

fn language(value: activity::Language) -> Result<wire::SourceDescriptionLanguage, Unsupported> {
    Ok(match value {
        activity::Language::TypeScript => wire::SourceDescriptionLanguage::TypeScript,
        activity::Language::Shell => wire::SourceDescriptionLanguage::Shell,
        activity::Language::Python => wire::SourceDescriptionLanguage::Python,
        _ => return Err(Unsupported),
    })
}

fn edit(value: &activity::PredictedEdit) -> Result<wire::SelectivePredictedEdit, Unsupported> {
    Ok(wire::SelectivePredictedEdit {
        snapshot: None,
        ordinal: value.ordinal,
        basis: match value.basis {
            activity::SelectiveEditBasis::StandardApiForecast => {
                wire::SelectiveEditBasis::StandardApiForecast
            }
            activity::SelectiveEditBasis::HostToolRequest => {
                wire::SelectiveEditBasis::HostToolRequest
            }
        },
        evidence: span(&value.evidence),
        details: match &value.details {
            activity::SelectiveEditDetails::Python {
                prediction,
                assumptions,
            } => wire::SelectiveEditDetails::Python {
                prediction: python(prediction)?,
                assumptions: assumptions.iter().copied().map(assumption).collect(),
            },
            activity::SelectiveEditDetails::JavaScript {
                prediction,
                assumptions,
            } => wire::SelectiveEditDetails::JavaScript {
                prediction: javascript::prediction(prediction)?,
                assumptions: assumptions
                    .iter()
                    .copied()
                    .map(javascript::assumption)
                    .collect::<Result<Vec<_>, _>>()?,
            },
            activity::SelectiveEditDetails::Patch { patch, cwd } => {
                wire::SelectiveEditDetails::Patch {
                    patch: wire::SubmittedPatch {
                        id: patch.id.index(),
                        invocation: span(&patch.invocation),
                        argument: span(&patch.argument),
                        payload: patch.payload.clone(),
                        files: patch
                            .files
                            .iter()
                            .map(file)
                            .collect::<Result<Vec<_>, _>>()?,
                    },
                    cwd: cwd.clone(),
                }
            }
        },
        predecessor_conditions: value.predecessor_conditions.iter().map(span).collect(),
        carrier_assumptions: value
            .carrier_assumptions
            .iter()
            .map(|carrier| {
                Ok(wire::SelectiveCarrierAssumption {
                    basis: match carrier.basis {
                        activity::SelectiveCarrierBasis::StandardShellDispatchAndStartup => {
                            wire::SelectiveCarrierBasis::StandardShellDispatchAndStartup
                        }
                        activity::SelectiveCarrierBasis::StandardPython3Cli => {
                            wire::SelectiveCarrierBasis::StandardPython3Cli
                        }
                        activity::SelectiveCarrierBasis::StandardNodeCommonJsCli => {
                            wire::SelectiveCarrierBasis::StandardNodeCommonJsCli
                        }
                    },
                    language: language(carrier.language)?,
                    invocation: span(&carrier.invocation),
                })
            })
            .collect::<Result<Vec<_>, _>>()?,
        later_context: match value.later_context {
            activity::SelectiveLaterContext::Unobserved => wire::SelectiveLaterContext::Unobserved,
            activity::SelectiveLaterContext::OpaqueSuffix => {
                wire::SelectiveLaterContext::OpaqueSuffix
            }
            activity::SelectiveLaterContext::KnownLaterTouch => {
                wire::SelectiveLaterContext::KnownLaterTouch
            }
            activity::SelectiveLaterContext::PossibleAliasLaterTouch => {
                wire::SelectiveLaterContext::PossibleAliasLaterTouch
            }
        },
    })
}

fn target(value: &activity::Target) -> Result<wire::SelectiveTarget, Unsupported> {
    match value {
        activity::Target::Literal { path, cwd } => Ok(wire::SelectiveTarget {
            path: path.clone(),
            cwd: cwd.clone(),
        }),
        _ => Err(Unsupported),
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod recipe_tests;
