//! Closed text/path interpreter for explicitly preloaded, trusted POSIX pathlib.

mod expressions;
mod text;

use crate::Basis;
use crate::Coverage;
use crate::Effect;
use crate::Language;
use crate::MAX_NESTING;
use crate::MAX_OPERATIONS;
use crate::Operation;
use crate::OperationId;
use crate::Report;
use crate::ReportVersion;
use crate::Request;
use crate::Source;
use crate::SourceId;
use crate::Target;
use crate::Transform;
use crate::Unresolved;
use crate::whole_script::AnalysisBudget;
use std::collections::BTreeMap;
use tree_sitter::Node;
use tree_sitter::Parser;

const MAX_TEXT: usize = crate::value::MAX_VALUE_BYTES - 64;
const MAX_BINDINGS: usize = 256;
const MAX_RETAINED: usize = 1_048_576;

#[derive(Clone)]
enum Value {
    Text(String),
    Path(String),
    Content {
        path: String,
        transforms: Vec<Transform>,
    },
    DiscardedWriteCount,
}

struct State<'a> {
    source: &'a str,
    cwd: Option<String>,
    constructor: String,
    bindings: BTreeMap<String, Value>,
    report: &'a mut Report,
    budget: AnalysisBudget,
}

pub(crate) fn analyze(parser: &mut Parser, request: Request, budget: AnalysisBudget) -> Report {
    let mut report = Report {
        version: ReportVersion::V1,
        call_id: request.call_id,
        coverage: Coverage::Opaque,
        sources: vec![Source {
            id: SourceId(0),
            language: Language::Python,
            parent: None,
        }],
        operations: vec![],
        unresolved: vec![],
        diffs: vec![],
    };
    let tree = crate::whole_script::parse(parser, &request.source, Some(budget));
    let Some(tree) = tree else {
        report.unresolved.push(Unresolved {
            evidence: crate::Span {
                source_id: SourceId(0),
                start_byte: 0,
                end_byte: request.source.len(),
                start_line: 1,
                end_line: 1,
                start_column: 1,
            },
            reason: "Trusted pathlib parsing did not complete".into(),
        });
        return report;
    };
    let root = tree.root_node();
    let mut state = State {
        source: &request.source,
        cwd: request.cwd,
        constructor: String::new(),
        bindings: BTreeMap::new(),
        report: &mut report,
        budget,
    };
    if request.source.contains('\0') || root.has_error() || state.run(root).is_none() {
        state.report.unresolved.push(Unresolved {
            evidence: crate::span(root, SourceId(0)),
            reason: "Source exceeds the bounded trusted pathlib text grammar or budget".into(),
        });
    }
    if !state.report.operations.is_empty() {
        state.report.coverage = Coverage::Partial;
    }
    report
}

impl State<'_> {
    fn run(&mut self, root: Node<'_>) -> Option<()> {
        let mut cursor = root.walk();
        let mut statements = root
            .named_children(&mut cursor)
            .filter(|n| n.kind() != "comment");
        self.constructor = import_name(statements.next()?, self.source)?;
        for statement in statements {
            if self.budget.expired() || statement.kind() != "expression_statement" {
                return None;
            }
            let parts = parts(statement);
            let [expression] = parts.as_slice() else {
                return None;
            };
            if expression.kind() == "assignment" {
                if expression.child_by_field_name("type").is_some() {
                    return None;
                }
                let left = expression.child_by_field_name("left")?;
                if left.kind() != "identifier"
                    || !identifier(crate::text(left, self.source))
                    || crate::text(left, self.source) == self.constructor
                {
                    return None;
                }
                let value = self.eval(expression.child_by_field_name("right")?, 0)?;
                if matches!(value, Value::DiscardedWriteCount) {
                    return None;
                }
                self.bindings
                    .insert(crate::text(left, self.source).to_owned(), value);
                if self.bindings.len() > MAX_BINDINGS || self.retained()? > MAX_RETAINED {
                    return None;
                }
            } else {
                self.eval(*expression, 0)?;
            }
        }
        Some(())
    }

    fn retained(&self) -> Option<usize> {
        self.bindings
            .iter()
            .try_fold(0usize, |total, (name, value)| {
                let payload = match value {
                    Value::Text(text) | Value::Path(text) => text.len(),
                    Value::Content { path, transforms } => {
                        path.len().checked_add(transforms.len().checked_mul(64)?)?
                    }
                    Value::DiscardedWriteCount => 0,
                };
                total
                    .checked_add(name.len())?
                    .checked_add(payload)?
                    .checked_add(64)
            })
    }

    fn emit(&mut self, node: Node<'_>, effect: Effect) -> Option<()> {
        if self.report.operations.len() >= MAX_OPERATIONS || self.budget.expired() {
            return None;
        }
        self.report.operations.push(Operation {
            id: OperationId(self.report.operations.len()),
            evidence: crate::span(node, SourceId(0)),
            basis: Basis::StaticIntent,
            effect,
        });
        Some(())
    }

    fn target(&self, path: String) -> Target {
        Target::Literal {
            path,
            cwd: self.cwd.clone(),
        }
    }
}

fn parts(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|n| n.kind() != "comment")
        .collect()
}

fn import_name(node: Node<'_>, source: &str) -> Option<String> {
    if node.kind() != "import_from_statement" {
        return None;
    }
    let tokens: Vec<_> = crate::text(node, source).split_whitespace().collect();
    let alias = match tokens.as_slice() {
        ["from", "pathlib", "import", "Path"] => "Path",
        ["from", "pathlib", "import", "Path", "as", alias] => alias,
        _ => return None,
    };
    let imported = parts(node);
    let [module, item] = imported.as_slice() else {
        return None;
    };
    if crate::text(*module, source) != "pathlib" {
        return None;
    }
    if item.kind() == "aliased_import" {
        if item.child_by_field_name("alias")?.kind() != "identifier" {
            return None;
        }
    } else if crate::text(*item, source) != "Path" {
        return None;
    }
    identifier(alias).then(|| alias.to_owned())
}

fn identifier(name: &str) -> bool {
    name != "__debug__"
        && name.is_ascii()
        && name
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}
