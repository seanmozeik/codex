//! Boolean selectors for discarded expressions, without general JS truthiness.

use super::Dependency;
use super::Kind;
use super::Reason;
use super::Result;
use super::State;
use super::Use;
use super::Value;
use super::syntax;
use super::syntax::BinaryOperator;
use tree_sitter::Node;

impl State<'_> {
    pub(super) fn lazy(
        &mut self,
        node: Node<'_>,
        operator: BinaryOperator,
        usage: Use,
    ) -> Result<Value> {
        if !matches!(usage, Use::Discard | Use::Condition) {
            return self.halt(node, Reason::UnsupportedOperation);
        }
        let left = self.expression(syntax::field(node, "left")?, Use::Condition)?;
        let Kind::Boolean(selector) = left.kind else {
            return self.halt(node, Reason::UnsupportedOperation);
        };
        self.retain_controls(&left.dependencies)?;
        let select_right = match operator {
            BinaryOperator::And => selector,
            BinaryOperator::Or => !selector,
            BinaryOperator::Add => return self.halt(node, Reason::UnsupportedOperation),
        };
        let result = if select_right {
            let right = self.expression(syntax::field(node, "right")?, usage)?;
            if usage == Use::Condition && !matches!(right.kind, Kind::Boolean(_)) {
                return self.halt(node, Reason::UnsupportedOperation);
            }
            let mut dependencies = left.dependencies;
            self.merge(&mut dependencies, right.dependencies)?;
            self.value(
                right.kind,
                dependencies,
                left.weight + right.weight + 64,
                left.depth.max(right.depth) + 1,
            )?
        } else {
            self.value(
                left.kind,
                left.dependencies,
                left.weight + 64,
                left.depth + 1,
            )?
        };
        // The enclosing expression completes after, never before, its RHS effects.
        self.condition(node)?;
        Ok(result)
    }

    fn retain_controls(&mut self, dependencies: &[Dependency]) -> Result<()> {
        let mut controls = std::mem::take(&mut self.control_dependencies);
        for dependency in dependencies {
            self.dependency(&mut controls, dependency.clone())?;
        }
        self.control_dependencies = controls;
        Ok(())
    }
}
