use super::State;
use crate::Language;
use crate::children;
use crate::text;
use crate::value::Value;
use crate::value::string_literal;
use tree_sitter::Node;

pub(super) fn bind(state: &mut State<'_, '_>, node: Node<'_>) {
    if state.language == Language::TypeScript && type_only(node) {
        state.gap(node, "Type-only import supplies no runtime binding");
        return;
    }
    if state.language == Language::Python {
        let module = node
            .child_by_field_name("module_name")
            .map(|n| text(n, state.source).to_owned());
        for item in children(node) {
            if node.child_by_field_name("module_name") == Some(item) {
                continue;
            }
            let (name, alias) = if item.kind() == "aliased_import" {
                (
                    item.child_by_field_name("name"),
                    item.child_by_field_name("alias"),
                )
            } else {
                (Some(item), None)
            };
            if let Some(name) = name {
                let name = text(name, state.source);
                let full = module
                    .as_ref()
                    .map_or_else(|| name.to_owned(), |m| format!("{m}.{name}"));
                let binding = alias.map_or(name, |n| text(n, state.source));
                let supported = matches!(
                    full.split('.').next(),
                    Some(
                        "pathlib" | "subprocess" | "re" | "os" | "shutil" | "json" | "math" | "sys"
                    )
                );
                let full = if module.is_none() && alias.is_none() {
                    full.split('.').next().unwrap_or(&full).to_owned()
                } else {
                    full
                };
                let binding = binding.split('.').next().unwrap_or(binding);
                state.bindings.insert(
                    binding.to_owned(),
                    if supported {
                        Value::Api(full)
                    } else {
                        Value::Unknown
                    },
                );
            }
        }
    } else if let Some(source) = node
        .child_by_field_name("source")
        .and_then(|n| string_literal(text(n, state.source), Language::TypeScript))
    {
        let module = normalize(&source);
        for clause in children(node)
            .into_iter()
            .filter(|n| n.kind() == "import_clause")
        {
            for item in children(clause) {
                match item.kind() {
                    "identifier" => {
                        state.bindings.insert(
                            text(item, state.source).to_owned(),
                            Value::Api(module.clone()),
                        );
                    }
                    "namespace_import" => {
                        if let Some(id) = item.named_child(0) {
                            state.bindings.insert(
                                text(id, state.source).to_owned(),
                                Value::Api(module.clone()),
                            );
                        }
                    }
                    "named_imports" => {
                        for spec in children(item) {
                            if type_only(spec) {
                                state.gap(spec, "Type-only import supplies no runtime binding");
                                continue;
                            }
                            if let Some(name) = spec.child_by_field_name("name") {
                                let alias = spec.child_by_field_name("alias").unwrap_or(name);
                                state.bindings.insert(
                                    text(alias, state.source).to_owned(),
                                    Value::Api(format!("{module}.{}", text(name, state.source))),
                                );
                            }
                        }
                    }
                    _ => state.gap(item, "Unsupported import binding"),
                }
            }
        }
    }
    state.gap(node, "Imported module initialization is not analysed");
}

pub(super) fn normalize(module: &str) -> String {
    match module {
        "fs" | "node:fs" | "fs/promises" | "node:fs/promises" => "fs".into(),
        "child_process" | "node:child_process" => "child_process".into(),
        "path" | "node:path" => "path".into(),
        other => format!("external:{other}"),
    }
}

pub(super) fn load(state: &mut State<'_, '_>, node: Node<'_>, input: &Value) -> Value {
    let Some(name) = input.text() else {
        state.gap(node, "Module name is unresolved");
        return Value::Unknown;
    };
    let module = normalize(&name);
    if matches!(module.as_str(), "fs" | "path" | "child_process") {
        Value::Api(module)
    } else {
        state.gap(node, "Module initialization and exports are not inspected");
        state.bindings.clear();
        Value::Unknown
    }
}

fn type_only(node: Node<'_>) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .any(|child| child.kind() == "type")
}
