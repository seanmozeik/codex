//! Statement ordering, binding versions, and canonical import handling.

use super::Binding;
use super::Kind;
use super::MAX_RECORDS;
use super::Result;
use super::State;
use super::Value;
use crate::SourceDescriptionFallback as Fallback;
use crate::selective_edits::python::PythonAssumption;
use crate::selective_edits::python::PythonBarrierReason;
use crate::selective_edits::python::PythonDependency;
use crate::selective_edits::python::syntax::field;
use crate::selective_edits::python::syntax::only;
use crate::selective_edits::python::syntax::parts;
use tree_sitter::Node;

impl State<'_> {
    pub(super) fn statement(&mut self, node: Node<'_>) -> Result<()> {
        self.guard.charge(0)?;
        match node.kind() {
            "pass_statement" => Ok(()),
            "for_statement" => self.literal_loop(node),
            "with_statement" => self.context(node),
            "import_statement" | "import_from_statement" => self.import(node),
            "function_definition" => {
                let name = crate::text(field(node, "name")?, self.source);
                self.bind(name, Value::new(Kind::DeferredFunction, Vec::new()), node)
            }
            "expression_statement" => {
                let expression = only(node)?;
                if expression.kind() == "assignment" {
                    // Python evaluates the RHS before assignment-target access.
                    let value = self.expression(field(expression, "right")?)?;
                    if value.has_handle() {
                        return self.halt(expression, PythonBarrierReason::EscapingHandle);
                    }
                    let target = field(expression, "left")?;
                    if target.kind() != "identifier" {
                        return self.halt(expression, PythonBarrierReason::UnsupportedOperation);
                    }
                    self.bind(crate::text(target, self.source), value, node)
                } else if expression.kind() == "augmented_assignment" {
                    let target = field(expression, "left")?;
                    // Unlike ordinary assignment, += looks up its LHS first.
                    // Capture that immutable value once, before any RHS I/O.
                    let left = self.expression(target)?;
                    if !matches!(&left.kind, Kind::Text(_)) {
                        return self.halt(expression, PythonBarrierReason::UnsupportedOperation);
                    }
                    let right = self.expression(field(expression, "right")?)?;
                    let value = self.concat_values(expression, left, right)?;
                    // A new binding version leaves aliases and read recipes
                    // captured from the old string unchanged.
                    self.bind(crate::text(target, self.source), value, node)
                } else {
                    let value = self.expression(expression)?;
                    // A discarded open handle does not escape. A stored/passed
                    // handle is stopped at its assignment or argument boundary.
                    if matches!(&value.kind, Kind::Method { .. }) && value.has_handle() {
                        return self.halt(expression, PythonBarrierReason::EscapingHandle);
                    }
                    Ok(())
                }
            }
            _ => Err(Fallback::UnsupportedSource.into()),
        }
    }

    pub(super) fn bind(&mut self, name: &str, value: Value, node: Node<'_>) -> Result<()> {
        if self.is_context_owner(name) {
            return self.halt(node, PythonBarrierReason::EscapingHandle);
        }
        if self.next_binding > MAX_RECORDS {
            return Err(Fallback::SourceLimit.into());
        }
        self.guard.charge(name.len() + 128)?;
        let binding = Binding {
            value,
            version: self.next_binding,
            evidence: self.evidence(node),
        };
        self.next_binding += 1;
        self.bindings.insert(name.to_owned(), binding);
        Ok(())
    }

    fn import(&mut self, node: Node<'_>) -> Result<()> {
        let mut items = parts(node);
        let kind = if node.kind() == "import_from_statement" {
            let module = items.next().ok_or(Fallback::UnsupportedSource)?;
            if crate::text(module, self.source) != "pathlib" {
                return self.halt(node, PythonBarrierReason::UnmodeledImport);
            }
            Kind::PathFunction
        } else {
            Kind::PathlibModule
        };
        let item = items.next().ok_or(Fallback::UnsupportedSource)?;
        if items.next().is_some() {
            return self.halt(node, PythonBarrierReason::UnmodeledImport);
        }
        let (original, alias) = if item.kind() == "aliased_import" {
            (field(item, "name")?, field(item, "alias")?)
        } else {
            (item, item)
        };
        let wanted = if matches!(&kind, Kind::PathFunction) {
            "Path"
        } else {
            "pathlib"
        };
        if crate::text(original, self.source) != wanted {
            return self.halt(node, PythonBarrierReason::UnmodeledImport);
        }
        if !self
            .plan
            .assumptions
            .contains(&PythonAssumption::CanonicalPathlib)
        {
            self.plan
                .assumptions
                .push(PythonAssumption::CanonicalPathlib);
        }
        let dependencies = vec![PythonDependency::Capability {
            assumption: PythonAssumption::CanonicalPathlib,
        }];
        self.guard.charge(256)?;
        self.bind(
            crate::text(alias, self.source),
            Value::new(kind, dependencies),
            node,
        )?;
        self.condition(node)
    }
}
