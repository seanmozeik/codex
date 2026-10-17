//! Bounded source evaluation with immutable values and explicit stop barriers.

mod calls;
mod conditionals;
mod contexts;
mod expressions;
mod join;
pub(super) mod loops;
mod pure;
mod statements;
mod strings;
mod summaries;
mod values;

use super::PythonAssumption;
use super::PythonBarrier;
use super::PythonBarrierReason;
use super::PythonDependency;
use super::PythonEdit;
use super::PythonExtraction;
use super::PythonFinalState;
use super::PythonOperation;
use super::PythonPlan;
use super::PythonSuffixTouch;
use super::syntax::parts;
use crate::SourceDescriptionFallback as Fallback;
use crate::SourceId;
use crate::Span;
use crate::Target;
use crate::selective_edits::PythonWriteEndpoint;
use crate::source_description::DescriptionBudget;
use contexts::ContextHandle;
use std::collections::BTreeMap;
use tree_sitter::Node;
use values::Binding;
use values::Kind;
use values::Method;
use values::Mode;
use values::TextValue;
use values::Value;

const MAX_RECORDS: usize = 256;
const MAX_RETAINED_BYTES: usize = 1_048_576;
const MAX_RECIPE_DEPTH: usize = 64;

enum Stop {
    Barrier,
    Unavailable(Fallback),
}

impl From<Fallback> for Stop {
    fn from(value: Fallback) -> Self {
        Self::Unavailable(value)
    }
}

type Result<T> = std::result::Result<T, Stop>;

struct Guard {
    budget: DescriptionBudget,
    charged: usize,
}

impl Guard {
    fn charge(&mut self, bytes: usize) -> std::result::Result<(), Fallback> {
        self.budget.check()?;
        self.charged = self
            .charged
            .checked_add(bytes)
            .ok_or(Fallback::SourceLimit)?;
        if self.charged > MAX_RETAINED_BYTES {
            return Err(Fallback::SourceLimit);
        }
        Ok(())
    }
}

struct State<'s> {
    source: &'s str,
    source_id: SourceId,
    cwd: Option<&'s str>,
    guard: Guard,
    plan: PythonPlan,
    suffix_touches: Vec<PythonSuffixTouch>,
    python_write_endpoint: PythonWriteEndpoint,
    bindings: BTreeMap<String, Binding>,
    next_binding: usize,
    next_read: usize,
    filesystem_epoch: usize,
    iteration_context: Vec<super::PythonIterationContext>,
    loop_iterations: usize,
    context_handles: Vec<ContextHandle>,
    logical_endpoint: bool,
}

pub(super) fn extract(
    root: Node<'_>,
    source: &str,
    source_id: SourceId,
    cwd: Option<&str>,
    endpoint: PythonWriteEndpoint,
    budget: DescriptionBudget,
) -> std::result::Result<PythonExtraction, Fallback> {
    let mut state = State {
        source,
        source_id,
        cwd,
        guard: Guard {
            budget,
            charged: source.len(),
        },
        plan: PythonPlan {
            edits: Vec::new(),
            barriers: Vec::new(),
            assumptions: vec![
                PythonAssumption::StandardPythonEvaluation,
                PythonAssumption::StandardBuiltinOpen,
                PythonAssumption::StandardTextMethods,
                PythonAssumption::NoConcurrentMutation,
            ],
            completion_conditions: Vec::new(),
        },
        suffix_touches: Vec::new(),
        python_write_endpoint: endpoint,
        bindings: BTreeMap::new(),
        next_binding: 1,
        next_read: 0,
        filesystem_epoch: 0,
        iteration_context: Vec::new(),
        loop_iterations: 0,
        context_handles: Vec::new(),
        logical_endpoint: false,
    };
    for statement in parts(root) {
        match state.statement(statement) {
            Ok(()) => {}
            Err(Stop::Barrier) => break,
            Err(Stop::Unavailable(reason)) => return Err(reason),
        }
    }
    budget.check()?;
    Ok(PythonExtraction {
        plan: state.plan,
        suffix_touches: state.suffix_touches,
    })
}

impl State<'_> {
    fn evidence(&self, node: Node<'_>) -> Span {
        crate::span(node, self.source_id)
    }

    fn halt<T>(&mut self, node: Node<'_>, reason: PythonBarrierReason) -> Result<T> {
        self.guard.charge(128)?;
        self.plan.barriers.push(PythonBarrier {
            evidence: self.evidence(node),
            reason,
        });
        for edit in &mut self.plan.edits {
            edit.final_state = PythonFinalState::OpaqueSuccessor;
        }
        Err(Stop::Barrier)
    }

    fn condition(&mut self, node: Node<'_>) -> Result<()> {
        if self.plan.completion_conditions.len() >= MAX_RECORDS {
            return Err(Fallback::SourceLimit.into());
        }
        self.guard.charge(64)?;
        self.plan.completion_conditions.push(self.evidence(node));
        Ok(())
    }

    fn dependency(
        &mut self,
        dependencies: &mut Vec<PythonDependency>,
        value: PythonDependency,
    ) -> Result<()> {
        if !dependencies.contains(&value) {
            if dependencies.len() >= MAX_RECORDS {
                return Err(Fallback::SourceLimit.into());
            }
            self.guard.charge(dependency_weight(&value))?;
            dependencies.push(value);
        }
        Ok(())
    }

    fn merge(
        &mut self,
        into: &mut Vec<PythonDependency>,
        values: Vec<PythonDependency>,
    ) -> Result<()> {
        for value in values {
            self.dependency(into, value)?;
        }
        Ok(())
    }

    fn copy_dependencies(&mut self, values: &[PythonDependency]) -> Result<Vec<PythonDependency>> {
        self.guard
            .charge(values.iter().map(dependency_weight).sum())?;
        Ok(values.to_vec())
    }

    fn emit(
        &mut self,
        node: Node<'_>,
        target: Target,
        operation: PythonOperation,
        mut dependencies: Vec<PythonDependency>,
    ) -> Result<()> {
        if self.plan.edits.len() >= MAX_RECORDS {
            return Err(Fallback::SourceLimit.into());
        }
        self.dependency(
            &mut dependencies,
            PythonDependency::Capability {
                assumption: PythonAssumption::StandardPythonEvaluation,
            },
        )?;
        self.dependency(
            &mut dependencies,
            PythonDependency::Capability {
                assumption: PythonAssumption::NoConcurrentMutation,
            },
        )?;
        self.dependency(
            &mut dependencies,
            PythonDependency::FilesystemEpoch {
                epoch: self.filesystem_epoch,
            },
        )?;
        self.guard.charge(
            256 + self.plan.completion_conditions.len() * 64 + self.iteration_context.len() * 128,
        )?;
        for earlier in &mut self.plan.edits {
            if earlier.target == target {
                earlier.final_state = PythonFinalState::KnownLaterTouch;
            } else if earlier.final_state == PythonFinalState::NotEstablished {
                earlier.final_state = PythonFinalState::PossibleAliasLaterTouch;
            }
        }
        let physical = !matches!(&operation, PythonOperation::LogicalWrite(_));
        self.plan.edits.push(PythonEdit {
            ordinal: self.plan.edits.len(),
            iteration_context: self.iteration_context.clone(),
            evidence: self.evidence(node),
            target,
            operation,
            dependencies,
            predecessor_conditions: self.plan.completion_conditions.clone(),
            final_state: PythonFinalState::NotEstablished,
        });
        if physical {
            self.filesystem_epoch += 1;
        }
        Ok(())
    }
}

fn target_weight(target: &Target) -> usize {
    match target {
        Target::Literal { path, cwd } => 64 + path.len() + cwd.as_ref().map_or(0, String::len),
        Target::Unresolved => 64,
    }
}

fn dependency_weight(value: &PythonDependency) -> usize {
    match value {
        PythonDependency::Binding { name, .. } => 128 + name.len(),
        PythonDependency::EntryWorkingDirectory { cwd } => {
            128 + cwd.as_ref().map_or(0, String::len)
        }
        _ => 128,
    }
}
