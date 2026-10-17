//! Ordered collection with one context and budget across all owned sources.

mod collection;

use super::PredictedEdit;
use super::PythonWriteEndpoint;
use super::SelectiveCarrierAssumption;
use super::SelectiveCarrierBasis;
use super::SelectiveEditAnalysis;
use super::SelectiveEditBasis;
use super::SelectiveEditDetails;
use super::SelectiveEditDisposition;
use super::SelectiveEditHostProfile;
use super::SelectiveEditHostProfileAnalysis;
use super::SelectiveEditInput;
use super::SelectiveLaterContext;
use super::input::SourceKind;
use super::javascript;
use super::orchestration;
use super::orchestration::CarrierBarrier;
use super::orchestration::CarrierEvent;
use super::python;
use super::shell;
use crate::Analyzer;
use crate::SourceDescriptionFallback;
use crate::Span;
use crate::source_description::DescriptionBudget;

const MAX_SOURCES: usize = 32;
const MAX_DEPTH: usize = 8;
const MAX_EDITS: usize = 256;
const MAX_BARRIERS: usize = 256;
const MAX_RETAINED_BYTES: usize = 1_048_576;

struct Collection {
    result: SelectiveEditAnalysis,
    decoded_bytes: usize,
    source_limit: usize,
    patch_count: usize,
    host_profile: Option<SelectiveEditHostProfile>,
    suffix_touches: Vec<(usize, Span)>,
    python_write_endpoint: PythonWriteEndpoint,
}

impl Analyzer {
    /// Forecasts independent edit operations under explicitly returned assumptions.
    ///
    /// Complete source is inspected before any forecast is published. Unknown
    /// effects inhibit dependent successors while earlier understood operations
    /// may survive, with final-state uncertainty recorded separately. This is a
    /// conditional UI forecast: it neither attests runtime identity nor reads,
    /// executes, authorizes, or changes any submitted command or file.
    #[must_use]
    pub fn forecast_edits(&mut self, input: SelectiveEditInput) -> SelectiveEditAnalysis {
        self.forecast_edits_inner(input, None)
    }

    /// Forecasts under additional caller-supplied whole-cell host premises.
    ///
    /// Selecting the profile asserts the assumptions documented on its variant,
    /// not an observed host identity or successful tool/display completion.
    /// The labelled return value is separate from ordinary UI forecasts.
    #[must_use]
    pub fn forecast_edits_with_host_profile(
        &mut self,
        input: SelectiveEditInput,
        profile: SelectiveEditHostProfile,
    ) -> SelectiveEditHostProfileAnalysis {
        SelectiveEditHostProfileAnalysis {
            host_profile: profile,
            analysis: self.forecast_edits_inner(input, Some(profile)),
        }
    }

    fn forecast_edits_inner(
        &mut self,
        input: SelectiveEditInput,
        host_profile: Option<SelectiveEditHostProfile>,
    ) -> SelectiveEditAnalysis {
        let budget = DescriptionBudget::new(input.limits.elapsed());
        let call_id = input.call_id.clone();
        let mut collection = Collection {
            result: SelectiveEditAnalysis::empty(input.call_id, SelectiveEditDisposition::Analyzed),
            decoded_bytes: 0,
            source_limit: input
                .limits
                .source_bytes()
                .min(crate::GUARD_MAX_SOURCE_BYTES),
            patch_count: 0,
            host_profile,
            suffix_touches: Vec::new(),
            python_write_endpoint: input.python_write_endpoint,
        };
        let outcome = self.forecast_source(
            input.kind,
            &input.source,
            None,
            input.cwd.as_deref(),
            &[],
            &[],
            0,
            budget,
            &mut collection,
        );
        match outcome.and_then(|_| collection.finish(budget)) {
            Ok(()) => collection.result,
            Err(reason) => {
                SelectiveEditAnalysis::empty(call_id, SelectiveEditDisposition::Unavailable(reason))
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn forecast_source(
        &mut self,
        kind: SourceKind,
        source: &str,
        parent: Option<Span>,
        cwd: Option<&str>,
        predecessors: &[Span],
        carriers: &[SelectiveCarrierAssumption],
        depth: usize,
        budget: DescriptionBudget,
        collection: &mut Collection,
    ) -> Result<bool, SourceDescriptionFallback> {
        budget.check()?;
        if depth > MAX_DEPTH {
            return Err(SourceDescriptionFallback::SourceLimit);
        }
        let id = collection.source(kind.language(), source, parent)?;
        if kind == SourceKind::Python {
            let python::PythonExtraction {
                plan,
                suffix_touches,
            } = python::extract(
                &mut self.python,
                source,
                id,
                cwd,
                collection.python_write_endpoint,
                budget,
            )?;
            let complete = plan.barriers.is_empty();
            let first_edit = collection.result.edits.len();
            for prediction in plan.edits {
                collection.push(PredictedEdit {
                    ordinal: 0,
                    basis: SelectiveEditBasis::StandardApiForecast,
                    evidence: prediction.evidence.clone(),
                    details: SelectiveEditDetails::Python {
                        prediction,
                        assumptions: plan.assumptions.clone(),
                    },
                    predecessor_conditions: predecessors.to_vec(),
                    carrier_assumptions: carriers.to_vec(),
                    later_context: SelectiveLaterContext::Unobserved,
                })?;
            }
            if collection.result.python_barriers.len() + plan.barriers.len() > MAX_BARRIERS {
                return Err(SourceDescriptionFallback::SourceLimit);
            }
            collection.result.python_barriers.extend(plan.barriers);
            collection.register_suffix_touches(first_edit, &suffix_touches);
            if !complete {
                collection.opaque_suffix();
            }
            return Ok(complete);
        }
        if kind == SourceKind::NodeCommonJs {
            let plan = javascript::extract(&mut self.typescript, source, id, cwd, budget)?;
            let complete = plan.barriers.is_empty();
            for prediction in plan.edits {
                collection.push(PredictedEdit {
                    ordinal: 0,
                    basis: SelectiveEditBasis::StandardApiForecast,
                    evidence: prediction.evidence.clone(),
                    details: SelectiveEditDetails::JavaScript {
                        prediction,
                        assumptions: plan.assumptions.clone(),
                    },
                    predecessor_conditions: predecessors.to_vec(),
                    carrier_assumptions: carriers.to_vec(),
                    later_context: SelectiveLaterContext::Unobserved,
                })?;
            }
            if collection.result.javascript_barriers.len() + plan.barriers.len() > MAX_BARRIERS {
                return Err(SourceDescriptionFallback::SourceLimit);
            }
            collection.result.javascript_barriers.extend(plan.barriers);
            if !complete {
                collection.opaque_suffix();
            }
            return Ok(complete);
        }
        let plan = match kind {
            SourceKind::CodeMode => orchestration::extract(
                &mut self.typescript,
                source,
                id,
                cwd,
                budget,
                collection.host_profile,
            )?,
            SourceKind::Shell => shell::extract(&mut self.preview_shell, source, id, cwd, budget)?,
            SourceKind::Python | SourceKind::NodeCommonJs => unreachable!("handled above"),
        };
        let mut conditions = predecessors.to_vec();
        for event in plan.events {
            budget.check()?;
            match event {
                CarrierEvent::Opaque { evidence, reason } => {
                    collection.barrier(evidence, carrier_reason(reason).into())?;
                    return Ok(false);
                }
                CarrierEvent::PatchDisplayCompleted { evidence } => {
                    // Reached only after the preceding patch parsed successfully.
                    // Completion is a prerequisite, never another edit or success claim.
                    conditions.push(evidence);
                }
                CarrierEvent::Patch {
                    payload,
                    invocation,
                    argument,
                    cwd,
                } => {
                    collection.charge_source(payload.len())?;
                    let patch = match crate::source_description::parse_selective_patch(
                        payload,
                        invocation.clone(),
                        argument,
                        collection.patch_count,
                        budget,
                    ) {
                        Ok(patch) => patch,
                        Err(SourceDescriptionFallback::UnsupportedSource) => {
                            collection.barrier(invocation, "unresolvedPatch".into())?;
                            return Ok(false);
                        }
                        Err(reason) => return Err(reason),
                    };
                    collection.patch_count += 1;
                    collection.push(PredictedEdit {
                        ordinal: 0,
                        basis: SelectiveEditBasis::HostToolRequest,
                        evidence: invocation.clone(),
                        details: SelectiveEditDetails::Patch { patch, cwd },
                        predecessor_conditions: conditions.clone(),
                        carrier_assumptions: carriers.to_vec(),
                        later_context: SelectiveLaterContext::Unobserved,
                    })?;
                    conditions.push(invocation);
                }
                CarrierEvent::Child {
                    kind,
                    text,
                    evidence,
                    cwd,
                    invocation,
                } => {
                    let mut nested_carriers = carriers.to_vec();
                    nested_carriers.push(SelectiveCarrierAssumption {
                        basis: SelectiveCarrierBasis::StandardShellDispatchAndStartup,
                        language: kind.language(),
                        invocation: invocation.clone(),
                    });
                    let interpreter_basis = match kind {
                        SourceKind::Python => Some(SelectiveCarrierBasis::StandardPython3Cli),
                        SourceKind::NodeCommonJs => {
                            Some(SelectiveCarrierBasis::StandardNodeCommonJsCli)
                        }
                        SourceKind::CodeMode | SourceKind::Shell => None,
                    };
                    if let Some(basis) = interpreter_basis {
                        nested_carriers.push(SelectiveCarrierAssumption {
                            basis,
                            language: kind.language(),
                            invocation: invocation.clone(),
                        });
                    }
                    let complete = match self.forecast_source(
                        kind,
                        &text,
                        Some(evidence),
                        cwd.as_deref(),
                        &conditions,
                        &nested_carriers,
                        depth + 1,
                        budget,
                        collection,
                    ) {
                        Ok(complete) => complete,
                        Err(SourceDescriptionFallback::UnsupportedSource)
                        | Err(SourceDescriptionFallback::UnsupportedLanguage) => {
                            collection.barrier(invocation.clone(), "unresolvedChild".into())?;
                            false
                        }
                        Err(reason) => return Err(reason),
                    };
                    if !complete {
                        collection.opaque_suffix();
                        return Ok(false);
                    }
                    conditions.push(invocation);
                }
            }
            if conditions.len() > MAX_EDITS {
                return Err(SourceDescriptionFallback::SourceLimit);
            }
        }
        budget.check()?;
        Ok(true)
    }
}

fn carrier_reason(reason: CarrierBarrier) -> &'static str {
    match reason {
        CarrierBarrier::UnknownExecution => "unknownExecution",
        CarrierBarrier::UnsupportedControl => "unsupportedControl",
        CarrierBarrier::UnresolvedDispatch => "unresolvedDispatch",
        CarrierBarrier::UnresolvedArguments => "unresolvedArguments",
        CarrierBarrier::ContextMutation => "contextMutation",
        CarrierBarrier::ConcurrentExecution => "concurrentExecution",
    }
}
