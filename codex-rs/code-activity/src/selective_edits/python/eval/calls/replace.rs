//! Existing replacement recipes separated from I/O signatures.

use super::Argument;
use crate::SourceDescriptionFallback as Fallback;
use crate::selective_edits::python::PythonAssumption;
use crate::selective_edits::python::PythonBarrierReason;
use crate::selective_edits::python::PythonDependency;
use crate::selective_edits::python::TextRecipe;
use crate::selective_edits::python::eval::Kind;
use crate::selective_edits::python::eval::MAX_RECIPE_DEPTH;
use crate::selective_edits::python::eval::Result;
use crate::selective_edits::python::eval::State;
use crate::selective_edits::python::eval::TextValue;
use crate::selective_edits::python::eval::Value;
use tree_sitter::Node;

impl State<'_> {
    pub(super) fn replace(
        &mut self,
        node: Node<'_>,
        receiver: Value,
        arguments: Vec<Argument>,
        mut dependencies: Vec<PythonDependency>,
    ) -> Result<Value> {
        if !(2..=3).contains(&arguments.len()) || arguments.iter().any(|arg| arg.name.is_some()) {
            return self.halt(node, PythonBarrierReason::UnsupportedOperation);
        }
        let count = if arguments.len() == 3 {
            match &arguments[2].value.kind {
                Kind::Integer(Some(value)) if *value >= 0 => {
                    Some(usize::try_from(*value).map_err(|_| Fallback::SourceLimit)?)
                }
                _ => return self.halt(node, PythonBarrierReason::UnsupportedOperation),
            }
        } else {
            None
        };
        let Kind::Text(input) = receiver.kind else {
            return Err(Fallback::UnsupportedSource.into());
        };
        // Keep the original wire recipe whenever both arguments have only
        // literal/concatenation origins, including the v7 literal-only subset.
        let (Some(old), Some(new)) = (
            arguments[0].value.constant_text(),
            arguments[1].value.constant_text(),
        ) else {
            return self.replace_text(
                node,
                input,
                &arguments[0].value,
                &arguments[1].value,
                count,
                dependencies,
            );
        };
        if input.depth >= MAX_RECIPE_DEPTH {
            return Err(Fallback::SourceLimit.into());
        }
        self.guard.charge(old.len() + new.len() + 128)?;
        self.dependency(
            &mut dependencies,
            PythonDependency::Capability {
                assumption: PythonAssumption::StandardTextMethods,
            },
        )?;
        let weight = input.weight + old.len() + new.len() + 128;
        let recipe = TextRecipe::Replace {
            input: Box::new(input.recipe),
            old,
            new,
            count,
            evidence: self.evidence(node),
        };
        Ok(Value::new(
            Kind::Text(TextValue {
                recipe,
                depth: input.depth + 1,
                weight,
            }),
            dependencies,
        ))
    }
}
