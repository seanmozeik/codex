//! Finite source-literal iteration. No iterator protocol or Python code runs.

use super::super::PythonIterationContext;
use super::Fallback;
use super::Result;
use super::State;
use crate::selective_edits::python::syntax::field;
use crate::selective_edits::python::syntax::parts;
use crate::selective_edits::python::syntax::unparenthesized;
use tree_sitter::Node;

pub(crate) const MAX_LOOP_DEPTH: usize = 4;
const MAX_LITERAL_ITEMS: usize = 64;
const MAX_UNPACK_NAMES: usize = 16;
const MAX_ITERATIONS: usize = 256;

pub(crate) struct Shape<'t> {
    pub(crate) names: Vec<Node<'t>>,
    pub(crate) rows: Vec<Vec<Node<'t>>>,
    pub(crate) body: Node<'t>,
}

/// Shared by the whole-source checker and evaluator. The checker additionally
/// validates names, literal decoding, all body statements and indentation.
pub(crate) fn shape<'t>(node: Node<'t>, source: &str) -> std::result::Result<Shape<'t>, Fallback> {
    if node.kind() != "for_statement" || parts(node).count() != 3 {
        return Err(Fallback::UnsupportedSource);
    }
    let mut cursor = node.walk();
    if node
        .children(&mut cursor)
        .any(|child| child.kind() == "async")
        || node.child_by_field_name("alternative").is_some()
    {
        return Err(Fallback::UnsupportedSource);
    }
    let target = field(node, "left")?;
    let unpack = target.kind() != "identifier";
    let names = if unpack {
        if !matches!(
            target.kind(),
            "pattern_list" | "tuple_pattern" | "list_pattern"
        ) {
            return Err(Fallback::UnsupportedSource);
        }
        if parts(target).count() > MAX_UNPACK_NAMES {
            return Err(Fallback::SourceLimit);
        }
        parts(target).collect::<Vec<_>>()
    } else {
        vec![target]
    };
    if names.is_empty() || names.iter().any(|name| name.kind() != "identifier") {
        return Err(Fallback::UnsupportedSource);
    }
    // Some grammar versions represent `(name)` as a tuple_pattern. Python
    // treats it as a scalar assignment target unless it contains a comma.
    // Conservatively refuse that wrapper instead of inventing unpacking.
    if target.kind() == "tuple_pattern" && names.len() == 1 {
        let mut cursor = target.walk();
        if !target
            .children(&mut cursor)
            .any(|child| child.kind() == ",")
        {
            return Err(Fallback::UnsupportedSource);
        }
    }
    for (index, name) in names.iter().enumerate() {
        if names[..index]
            .iter()
            .any(|earlier| crate::text(*earlier, source) == crate::text(*name, source))
        {
            return Err(Fallback::UnsupportedSource);
        }
    }
    let iterable = unparenthesized(field(node, "right")?)?;
    if !matches!(iterable.kind(), "list" | "tuple") {
        return Err(Fallback::UnsupportedSource);
    }
    if parts(iterable).count() > MAX_LITERAL_ITEMS {
        return Err(Fallback::SourceLimit);
    }
    let mut rows = Vec::new();
    for item in parts(iterable) {
        let item = unparenthesized(item)?;
        let row = if unpack {
            if item.kind() != "tuple" || parts(item).count() != names.len() {
                return Err(Fallback::UnsupportedSource);
            }
            parts(item)
                .map(unparenthesized)
                .collect::<std::result::Result<Vec<_>, _>>()?
        } else {
            vec![item]
        };
        if row.iter().any(|value| value.kind() != "string") {
            return Err(Fallback::UnsupportedSource);
        }
        rows.push(row);
    }
    let body = field(node, "body")?;
    if body.kind() != "block" || parts(body).next().is_none() {
        return Err(Fallback::UnsupportedSource);
    }
    Ok(Shape { names, rows, body })
}

impl State<'_> {
    pub(super) fn literal_loop(&mut self, node: Node<'_>) -> Result<()> {
        self.guard.charge(0)?;
        if self.iteration_context.len() >= MAX_LOOP_DEPTH {
            return Err(Fallback::SourceLimit.into());
        }
        let shape = shape(node, self.source)?;
        let literal_count = shape.rows.len();
        for (index, row) in shape.rows.into_iter().enumerate() {
            if self.loop_iterations >= MAX_ITERATIONS {
                return Err(Fallback::SourceLimit.into());
            }
            self.loop_iterations += 1;
            self.guard.charge(128)?;
            self.iteration_context.push(PythonIterationContext {
                evidence: self.evidence(node),
                index,
                literal_count,
            });
            // No rewind or fresh State: successive iterations must share binding
            // versions, predecessor conditions, read identities and file epochs.
            let result: Result<()> = (|| {
                for (name, item) in shape.names.iter().zip(row) {
                    let value = self.expression(item)?;
                    self.bind(crate::text(*name, self.source), value, *name)?;
                }
                for statement in parts(shape.body) {
                    self.statement(statement)?;
                }
                Ok(())
            })();
            self.iteration_context.pop();
            // A barrier or budget failure stops this and every enclosing loop.
            result?;
        }
        Ok(())
    }
}
