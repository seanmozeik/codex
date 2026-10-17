//! Existing code-point slices and split-item subscription, in evaluation order.

use crate::selective_edits::python::PythonAssumption;
use crate::selective_edits::python::PythonBarrierReason;
use crate::selective_edits::python::PythonDependency;
use crate::selective_edits::python::TextRecipe;
use crate::selective_edits::python::eval::Kind;
use crate::selective_edits::python::eval::Result;
use crate::selective_edits::python::eval::State;
use crate::selective_edits::python::eval::TextValue;
use crate::selective_edits::python::eval::Value;
use crate::selective_edits::python::syntax::field;
use crate::selective_edits::python::syntax::parts;
use tree_sitter::Node;

impl State<'_> {
    pub(in crate::selective_edits::python::eval) fn string_subscript(
        &mut self,
        node: Node<'_>,
    ) -> Result<Value> {
        let receiver = self.expression(field(node, "value")?)?;
        if receiver.has_handle() {
            return self.halt(node, PythonBarrierReason::EscapingHandle);
        }
        let subscript = field(node, "subscript")?;
        let mut dependencies = receiver.dependencies;
        if subscript.kind() == "slice" {
            // The grammar does not assign fields to slice bounds. Separate them
            // by the actual colon token, never by text searches inside literals.
            let mut cursor = subscript.walk();
            let colons: Vec<_> = subscript
                .children(&mut cursor)
                .filter(|child| child.kind() == ":")
                .collect();
            if colons.len() != 1 {
                return self.halt(node, PythonBarrierReason::UnsupportedOperation);
            }
            let mut start = None;
            let mut end = None;
            let mut bounds = Vec::new();
            for bound in parts(subscript) {
                let value = self.expression(bound)?;
                if value.has_handle() {
                    return self.halt(bound, PythonBarrierReason::EscapingHandle);
                }
                let copied = self.copy_dependencies(&value.dependencies)?;
                self.merge(&mut dependencies, copied)?;
                bounds.push((bound, value));
            }
            // Both bound expressions precede subscription conversion checks.
            for (bound, value) in bounds {
                let value = self.index_value(bound, &value, self.evidence(bound))?;
                if bound.end_byte() <= colons[0].start_byte() {
                    start = Some(value);
                } else {
                    end = Some(value);
                }
            }
            let Kind::Text(input) = receiver.kind else {
                return self.halt(node, PythonBarrierReason::UnsupportedOperation);
            };
            let depth = input
                .depth
                .max(start.as_ref().map_or(0, |v| v.depth))
                .max(end.as_ref().map_or(0, |v| v.depth))
                + 1;
            self.string_depth(depth)?;
            let weight = input.weight
                + start.as_ref().map_or(0, |v| v.weight)
                + end.as_ref().map_or(0, |v| v.weight)
                + 128;
            self.guard.charge(128)?;
            self.dependency(
                &mut dependencies,
                PythonDependency::Capability {
                    assumption: PythonAssumption::StandardTextMethods,
                },
            )?;
            return Ok(Value::new(
                Kind::Text(TextValue {
                    recipe: TextRecipe::Slice {
                        input: Box::new(input.recipe),
                        start: start.map(|v| Box::new(v.recipe)),
                        end: end.map(|v| Box::new(v.recipe)),
                        evidence: self.evidence(node),
                    },
                    depth,
                    weight,
                }),
                dependencies,
            ));
        }
        // Preserve Python's receiver-then-index order, including index I/O.
        let index = self.expression(subscript)?;
        if index.has_handle() {
            return self.halt(subscript, PythonBarrierReason::EscapingHandle);
        }
        self.merge(&mut dependencies, index.dependencies)?;
        let (Kind::ListText(split), Kind::Integer(Some(index))) = (receiver.kind, index.kind)
        else {
            return self.halt(node, PythonBarrierReason::UnsupportedOperation);
        };
        if let TextRecipe::Literal { value, .. } = &split.input.recipe {
            let possible_splits = value.matches(split.separator.as_str()).count();
            let splits = match split.maxsplit {
                Some(limit) if limit >= 0 => {
                    possible_splits.min(usize::try_from(limit).unwrap_or(usize::MAX))
                }
                _ => possible_splits,
            };
            let item_count = splits + 1;
            let out_of_range = if index >= 0 {
                usize::try_from(index).map_or(true, |index| index >= item_count)
            } else {
                index.unsigned_abs() > item_count as u64
            };
            if out_of_range {
                return self.halt(node, PythonBarrierReason::UnsupportedOperation);
            }
        }
        let depth = split.input.depth + 2;
        self.string_depth(depth)?;
        self.guard.charge(128)?;
        self.condition(node)?; // An unknown-length split may raise IndexError.
        Ok(Value::new(
            Kind::Text(TextValue {
                recipe: TextRecipe::SplitItem {
                    input: Box::new(split.input.recipe),
                    separator: split.separator,
                    maxsplit: split.maxsplit,
                    index,
                    split_evidence: split.evidence,
                    evidence: self.evidence(node),
                },
                depth,
                weight: split.weight + 128,
            }),
            dependencies,
        ))
    }
}
