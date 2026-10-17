//! Sequential lexical values, owned descriptors and bounded Node string recipes.

#[path = "calls.rs"]
mod calls;
#[path = "descriptors.rs"]
mod descriptors;
#[path = "lazy.rs"]
mod lazy;
#[path = "values.rs"]
mod values;

use super::JavaScriptAssumption as Assumption;
use super::JavaScriptBarrier;
use super::JavaScriptBarrierReason as Reason;
use super::JavaScriptDependency as Dependency;
use super::JavaScriptEdit;
use super::JavaScriptPlan;
use super::JavaScriptTextRecipe as Recipe;
use super::JavaScriptWriteEffect as WriteEffect;
use super::declarations;
use super::declarations::BindingTarget;
use super::declarations::FilesystemMethod;
use super::syntax;
use super::syntax::Mutability;
use crate::SourceDescriptionFallback as Fallback;
use crate::SourceId;
use crate::Span;
use crate::Target;
use crate::source_description::DescriptionBudget;
use descriptors::Descriptor;
use descriptors::DescriptorId;
use std::collections::BTreeMap;
use tree_sitter::Node;

const MAX_RECORDS: usize = 256;
const MAX_RETAINED: usize = 1_048_576;

enum Stop {
    Barrier,
    Unavailable(Fallback),
}

impl From<Fallback> for Stop {
    fn from(reason: Fallback) -> Self {
        Self::Unavailable(reason)
    }
}

type Result<T> = std::result::Result<T, Stop>;

#[derive(Clone)]
enum Kind {
    Text(Recipe),
    Filesystem,
    FilesystemFunction(FilesystemMethod),
    Descriptor(DescriptorId),
    Integer(usize),
    Boolean(bool),
    Null,
    Discarded,
}

#[derive(Clone)]
struct Value {
    kind: Kind,
    dependencies: Vec<Dependency>,
    weight: usize,
    depth: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Use {
    InitializeConst,
    Value,
    Condition,
    Discard,
}

#[derive(Clone)]
struct Binding {
    value: Value,
    version: usize,
    evidence: Span,
}

struct State<'source> {
    source: &'source str,
    source_id: SourceId,
    cwd: Option<&'source str>,
    budget: DescriptionBudget,
    charged: usize,
    declared: BTreeMap<String, Mutability>,
    bindings: BTreeMap<String, Binding>,
    control_dependencies: Vec<Dependency>,
    next_binding: usize,
    next_read: usize,
    next_descriptor: usize,
    descriptors: BTreeMap<DescriptorId, Descriptor>,
    plan: JavaScriptPlan,
}

pub(super) fn extract(
    root: Node<'_>,
    source: &str,
    source_id: SourceId,
    cwd: Option<&str>,
    budget: DescriptionBudget,
    declared: BTreeMap<String, Mutability>,
) -> std::result::Result<JavaScriptPlan, Fallback> {
    let mut state = State {
        source,
        source_id,
        cwd,
        budget,
        declared,
        charged: source.len(),
        bindings: BTreeMap::new(),
        control_dependencies: Vec::new(),
        next_binding: 1,
        next_read: 0,
        next_descriptor: 0,
        descriptors: BTreeMap::new(),
        plan: JavaScriptPlan {
            edits: Vec::new(),
            barriers: Vec::new(),
            completion_conditions: Vec::new(),
            assumptions: vec![
                Assumption::StandardNodeCommonJsEvaluation,
                Assumption::CanonicalNodeFs,
                Assumption::StandardStringMethods,
                Assumption::NoConcurrentMutation,
            ],
        },
    };
    for statement in syntax::parts(root) {
        match state.statement(statement) {
            Ok(()) => {}
            Err(Stop::Barrier) => break,
            Err(Stop::Unavailable(reason)) => return Err(reason),
        }
    }
    budget.check()?;
    Ok(state.plan)
}

impl State<'_> {
    fn charge(&mut self, bytes: usize) -> Result<()> {
        self.budget.check()?;
        self.charged = self
            .charged
            .checked_add(bytes)
            .ok_or(Fallback::SourceLimit)?;
        if self.charged > MAX_RETAINED {
            return Err(Fallback::SourceLimit.into());
        }
        Ok(())
    }

    fn evidence(&self, node: Node<'_>) -> Span {
        crate::span(node, self.source_id)
    }

    fn halt<T>(&mut self, node: Node<'_>, reason: Reason) -> Result<T> {
        self.charge(128)?;
        self.plan.barriers.push(JavaScriptBarrier {
            evidence: self.evidence(node),
            reason,
        });
        Err(Stop::Barrier)
    }

    fn statement(&mut self, node: Node<'_>) -> Result<()> {
        self.charge(0)?;
        if node.kind() == "empty_statement" {
            return Ok(());
        }
        if node.kind() == "lexical_declaration" {
            let declaration = declarations::parse(node, self.source)?;
            let usage = if declaration.mutable == Mutability::Const {
                Use::InitializeConst
            } else {
                Use::Value
            };
            let value = self.expression(declaration.initializer, usage)?;
            return match declaration.target {
                BindingTarget::Name(name) => self.bind(name, value, node),
                BindingTarget::FilesystemFunctions(bindings) => {
                    if !matches!(value.kind, Kind::Filesystem) {
                        return self.halt(node, Reason::UnsupportedOperation);
                    }
                    for (method, name) in bindings {
                        let function =
                            self.value(Kind::FilesystemFunction(method), Vec::new(), 128, 0)?;
                        self.bind(name, function, node)?;
                    }
                    Ok(())
                }
            };
        }
        let expression = syntax::only(node)?;
        if expression.kind() == "assignment_expression" {
            let name = crate::text(syntax::field(expression, "left")?, self.source);
            let value = self.expression(syntax::field(expression, "right")?, Use::Value)?;
            if self.declared.get(name) != Some(&Mutability::Let)
                || !self.bindings.contains_key(name)
                || matches!(
                    value.kind,
                    Kind::Filesystem | Kind::FilesystemFunction(_) | Kind::Descriptor(_)
                )
            {
                return self.halt(expression, Reason::UnsupportedOperation);
            }
            self.bind(name, value, node)
        } else {
            self.expression(expression, Use::Discard).map(|_| ())
        }
    }

    fn bind(&mut self, name: &str, value: Value, node: Node<'_>) -> Result<()> {
        if self.next_binding > MAX_RECORDS {
            return Err(Fallback::SourceLimit.into());
        }
        self.charge(name.len() + 128)?;
        let binding = Binding {
            value,
            version: self.next_binding,
            evidence: self.evidence(node),
        };
        self.next_binding += 1;
        self.bindings.insert(name.to_owned(), binding);
        Ok(())
    }

    fn condition(&mut self, node: Node<'_>) -> Result<()> {
        if self.plan.completion_conditions.len() >= MAX_RECORDS {
            return Err(Fallback::SourceLimit.into());
        }
        self.charge(64)?;
        self.plan.completion_conditions.push(self.evidence(node));
        Ok(())
    }

    fn emit(
        &mut self,
        node: Node<'_>,
        target: Target,
        content: Recipe,
        write_effect: WriteEffect,
        mut dependencies: Vec<Dependency>,
    ) -> Result<()> {
        if self.plan.edits.len() >= MAX_RECORDS {
            return Err(Fallback::SourceLimit.into());
        }
        self.merge(&mut dependencies, self.control_dependencies.clone())?;
        self.dependency(
            &mut dependencies,
            Dependency::EntryWorkingDirectory {
                cwd: self.cwd.map(str::to_owned),
            },
        )?;
        self.dependency(
            &mut dependencies,
            Dependency::FilesystemEpoch {
                epoch: self.plan.edits.len(),
            },
        )?;
        for assumption in self.plan.assumptions.clone() {
            self.dependency(&mut dependencies, Dependency::Capability { assumption })?;
        }
        self.charge(
            256 + self.plan.completion_conditions.len() * 64 + self.cwd.map_or(0, str::len),
        )?;
        self.plan.edits.push(JavaScriptEdit {
            ordinal: self.plan.edits.len(),
            evidence: self.evidence(node),
            target,
            content,
            write_effect,
            dependencies,
            predecessor_conditions: self.plan.completion_conditions.clone(),
        });
        self.condition(node)
    }
}
