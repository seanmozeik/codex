use crate::Basis;
use crate::Effect;
use crate::Language;
use crate::MAX_NESTING;
use crate::MAX_OPERATIONS;
use crate::Operation;
use crate::OperationId;
use crate::Report;
use crate::SourceId;
use crate::Span;
use crate::Unresolved;
use crate::children;
use crate::span;
use crate::text;
use crate::value::FunctionId;
use crate::value::MAX_VALUE_BYTES;
use crate::value::Value;
use crate::value::string_literal;
use functions::Flow;
use functions::Function;
use scopes::Bindings;
use tree_sitter::Node;

mod callbacks;
mod calls;
mod computed;
mod control;
mod data;
mod expressions;
mod function_locals;
mod functions;
mod imports;
mod literal_loop;
mod literal_replace;
mod scopes;
mod statements;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Diagnostic,
    Complete,
}

pub struct Embedded {
    pub source: String,
    pub language: Language,
    pub cwd: Option<String>,
    pub parent: Span,
}
pub struct State<'a, 'tree> {
    source: &'a str,
    source_id: SourceId,
    language: Language,
    mode: Mode,
    cwd: Option<String>,
    bindings: Bindings,
    functions: Vec<Function<'tree>>,
    active_calls: Vec<FunctionId>,
    flow: Flow,
    loop_context: literal_loop::CallContext,
    loop_source: literal_loop::SourcePolicy,
    report: &'a mut Report,
    embedded: Vec<Embedded>,
    visited: usize,
}

pub fn analyze(
    root: Node<'_>,
    source: &str,
    source_id: SourceId,
    language: Language,
    cwd: Option<String>,
    report: &mut Report,
    mode: Mode,
) -> Vec<Embedded> {
    let globals: &[&str] = match language {
        Language::Python => &[
            "open", "print", "str", "len", "int", "float", "abs", "round", "sum", "sorted", "list",
            "bool", "min", "max", "any", "all",
        ],
        Language::TypeScript => &[
            "require", "import", "Bun", "Deno", "JSON", "tools", "text", "console",
        ],
        Language::Shell => &[],
    };
    let loop_source = literal_loop::SourcePolicy::inspect(root, mode, language);
    let mut state = State {
        source,
        source_id,
        language,
        mode,
        cwd,
        bindings: Bindings::new(
            globals
                .iter()
                .map(|s| (s.to_string(), Value::Api(s.to_string())))
                .collect(),
        ),
        functions: vec![],
        active_calls: vec![],
        flow: Flow::Continue,
        loop_context: literal_loop::CallContext::Outside,
        loop_source,
        report,
        embedded: vec![],
        visited: 0,
    };
    if loop_source == literal_loop::SourcePolicy::TextOnly
        && !state.literal_loop_source_structure(root, /*depth*/ 0)
    {
        state.gap(
            root,
            "Strict Python loop sources require straight-line text/file statements",
        );
    } else {
        state.eval(root, /*depth*/ 0);
    }
    state.embedded
}

impl<'tree> State<'_, 'tree> {
    fn reserve_value(
        &mut self,
        node: Node<'_>,
        value: &Value,
        key_bytes: usize,
        remaining: &mut usize,
    ) -> bool {
        let size = value
            .budget_size()
            .and_then(|size| size.checked_add(key_bytes));
        if let Some(size) = size
            && size <= *remaining
        {
            *remaining -= size;
            return true;
        }
        self.gap(
            node,
            "Abstract value construction budget exceeded; bindings invalidated",
        );
        self.bindings.clear();
        self.cwd = None;
        false
    }

    fn gap(&mut self, node: Node<'_>, reason: &str) {
        if self.report.unresolved.len() < MAX_OPERATIONS {
            self.report.unresolved.push(Unresolved {
                evidence: span(node, self.source_id),
                reason: reason.into(),
            });
        }
    }
    fn emit(&mut self, node: Node<'_>, effect: Effect) {
        if self.report.operations.len() >= MAX_OPERATIONS {
            self.gap(node, "Operation budget exceeded");
            return;
        }
        self.report.operations.push(Operation {
            id: OperationId(self.report.operations.len()),
            evidence: span(node, self.source_id),
            basis: Basis::StaticIntent,
            effect,
        });
    }
    fn insert_binding(&mut self, node: Node<'_>, name: String, value: Value) {
        if !self.bindings.insert(name, value) {
            self.gap(
                node,
                "Abstract value or binding budget exceeded; bindings invalidated",
            );
        }
    }
    fn bind(&mut self, node: Node<'_>, value: Value) {
        if node.kind() == "object_pattern"
            && let Value::Api(api) = value
        {
            for property in children(node) {
                let (name, alias) = if property.kind() == "shorthand_property_identifier_pattern" {
                    (Some(property), Some(property))
                } else {
                    (
                        property.child_by_field_name("key"),
                        property.child_by_field_name("value"),
                    )
                };
                if let (Some(name), Some(alias)) = (name, alias) {
                    self.bind(
                        alias,
                        Value::Api(format!("{api}.{}", text(name, self.source))),
                    );
                } else {
                    self.gap(property, "Unsupported module destructuring");
                }
            }
        } else if matches!(
            node.kind(),
            "identifier" | "shorthand_property_identifier_pattern"
        ) {
            self.insert_binding(node, text(node, self.source).to_owned(), value);
        } else {
            self.gap(
                node,
                "Assignment target cannot be resolved; bindings invalidated",
            );
            self.bindings.clear();
        }
    }
    fn eval(&mut self, node: Node<'tree>, depth: usize) -> Value {
        self.visited += 1;
        if depth > MAX_NESTING || self.visited > 50_000 {
            self.gap(node, "AST traversal budget exceeded");
            return Value::Unknown;
        }
        let value = match node.kind() {
            "module"
            | "program"
            | "block"
            | "statement_block"
            | "lexical_declaration"
            | "expression_statement" => self.eval_sequence(node, depth),

            "variable_declaration" => {
                self.gap(
                    node,
                    "JavaScript var hoisting is not inspected; bindings invalidated",
                );
                self.bindings.clear();
                Value::Unknown
            }
            "function_definition"
            | "function_declaration"
            | "function_expression"
            | "arrow_function"
            | "lambda" => self.define_function(node, depth),
            "return_statement" => self.eval_return(node, depth),
            "import_statement" | "import_from_statement" => {
                imports::bind(self, node);
                Value::Unknown
            }
            "assignment" | "assignment_expression" | "variable_declarator" => {
                self.eval_assignment(node, depth)
            }
            "identifier" | "import" => self
                .bindings
                .get(text(node, self.source))
                .cloned()
                .unwrap_or(Value::Unknown),
            "string" => {
                if children(node).iter().any(|n| n.kind() == "interpolation") {
                    self.eval_format(node, depth)
                } else {
                    string_literal(text(node, self.source), self.language)
                        .map_or(Value::Unknown, Value::Text)
                }
            }
            "template_string" => self.eval_template(node, depth),
            "await_expression" => self.eval_await(node, depth),
            "parenthesized_expression" | "as_expression" | "non_null_expression" => node
                .named_child(0)
                .map_or(Value::Unknown, |n| self.eval(n, depth + 1)),
            "attribute" | "member_expression" => self.eval_member(node, depth),
            "call" | "call_expression" => self.eval_call(node, depth),
            "list" | "tuple" | "array" => self.eval_array(node, depth),
            "object" | "dictionary" => self.eval_object(node, depth),
            "binary_operator" | "binary_expression" | "boolean_operator" => {
                self.eval_binary(node, depth)
            }
            "subscript" | "subscript_expression" => self.eval_subscript(node, depth),
            "with_statement" => self.eval_with(node, depth),
            "for_statement" | "for_in_statement" => self.eval_loop(node, depth),
            "if_statement" => self.eval_branches(node, depth),
            "list_comprehension" => self.eval_comprehension(node, depth),
            "generator_expression" => self.eval_generator(node, depth),
            "assert_statement" | "comparison_operator" => {
                for part in children(node) {
                    self.eval(part, depth + 1);
                }
                Value::Data
            }
            "integer" | "float" | "number" | "none" | "null" | "regex" => Value::Data,
            "true" => Value::Boolean(true),
            "false" => Value::Boolean(false),
            "comment" | "type_annotation" | "interface_declaration" | "type_alias_declaration" => {
                Value::Unknown
            }
            _ => {
                self.gap(
                    node,
                    "Unsupported syntax or control flow; region skipped and bindings invalidated",
                );
                self.bindings.clear();
                Value::Unknown
            }
        };
        if node.kind() == "identifier" {
            self.reject_unknown_loop_identifier(node, &value);
        }
        let mut remaining = MAX_VALUE_BYTES;
        if self.reserve_value(node, &value, /*key_bytes*/ 0, &mut remaining) {
            value
        } else {
            Value::Unknown
        }
    }
}
