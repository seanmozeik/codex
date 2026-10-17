//! Expressions preserve text and path origins without generic API coercions.

use super::MAX_NESTING;
use super::MAX_TEXT;
use super::State;
use super::Value;
use super::parts;
use super::text;
use crate::Effect;
use crate::Language;
use crate::Transform;
use crate::WriteMode;
use tree_sitter::Node;

impl State<'_> {
    pub(super) fn eval(&mut self, node: Node<'_>, depth: usize) -> Option<Value> {
        if depth > MAX_NESTING || self.budget.expired() {
            return None;
        }
        match node.kind() {
            "string" => literal(node, self.source).map(Value::Text),
            "identifier" => self.bindings.get(crate::text(node, self.source)).cloned(),
            "parenthesized_expression" => {
                let items = parts(node);
                let [inner] = items.as_slice() else {
                    return None;
                };
                self.eval(*inner, depth + 1)
            }
            "call" => self.call(node, depth + 1),
            _ => None,
        }
    }

    fn call(&mut self, node: Node<'_>, depth: usize) -> Option<Value> {
        if depth > MAX_NESTING {
            return None;
        }
        let callee = node.child_by_field_name("function")?;
        let arguments = parts(node.child_by_field_name("arguments")?);
        if arguments.iter().any(|arg| {
            matches!(
                arg.kind(),
                "keyword_argument" | "list_splat" | "dictionary_splat"
            )
        }) {
            return None;
        }
        if callee.kind() == "identifier" && crate::text(callee, self.source) == self.constructor {
            let [argument] = arguments.as_slice() else {
                return None;
            };
            let path = literal(*argument, self.source)?;
            if path.contains('\0') {
                return None;
            }
            return Some(Value::Path(text::path(&path)));
        }
        if callee.kind() != "attribute" {
            return None;
        }
        let receiver = self.eval(callee.child_by_field_name("object")?, depth + 1)?;
        let method = crate::text(callee.child_by_field_name("attribute")?, self.source);
        match (receiver, method) {
            (Value::Path(path), "read_text") if arguments.is_empty() => {
                self.emit(
                    node,
                    Effect::FileRead {
                        target: self.target(path.clone()),
                    },
                )?;
                Some(Value::Content {
                    path,
                    transforms: vec![],
                })
            }
            (Value::Path(path), "write_text") => {
                let [argument] = arguments.as_slice() else {
                    return None;
                };
                let value = self.eval(*argument, depth + 1)?;
                let effect = match value {
                    Value::Content {
                        path: origin,
                        transforms,
                    } if origin == path && !transforms.is_empty() => Effect::FileEdit {
                        target: self.target(path),
                        transforms,
                    },
                    Value::Text(_) | Value::Content { .. } => Effect::FileWrite {
                        target: self.target(path),
                        mode: WriteMode::Replace,
                    },
                    _ => return None,
                };
                self.emit(node, effect)?;
                Some(Value::DiscardedWriteCount)
            }
            (Value::Text(receiver), "replace") => {
                let [old, new] = arguments.as_slice() else {
                    return None;
                };
                let Value::Text(old) = self.eval(*old, depth + 1)? else {
                    return None;
                };
                let Value::Text(new) = self.eval(*new, depth + 1)? else {
                    return None;
                };
                text::replace(&receiver, &old, &new).map(Value::Text)
            }
            (
                Value::Content {
                    path,
                    mut transforms,
                },
                "replace",
            ) => {
                let (old, new) = match arguments.as_slice() {
                    [old, new] => (*old, *new),
                    [old, new, count]
                        if count.kind() == "integer" && crate::text(*count, self.source) == "1" =>
                    {
                        (*old, *new)
                    }
                    _ => return None,
                };
                let Value::Text(_) = self.eval(old, depth + 1)? else {
                    return None;
                };
                let Value::Text(_) = self.eval(new, depth + 1)? else {
                    return None;
                };
                if transforms.len() >= 32 {
                    return None;
                }
                transforms.push(Transform::TextReplacement);
                Some(Value::Content { path, transforms })
            }
            (
                Value::Content {
                    path,
                    mut transforms,
                },
                "strip",
            ) if arguments.is_empty() => {
                if transforms.len() >= 32 {
                    return None;
                }
                transforms.push(Transform::TextNormalization);
                Some(Value::Content { path, transforms })
            }
            _ => None,
        }
    }
}

fn literal(node: Node<'_>, source: &str) -> Option<String> {
    if node.kind() != "string" {
        return None;
    }
    let raw = crate::text(node, source);
    // Python normalizes source newlines; its unknown backtick escape differs
    // from the shared decoder. Abstain instead of creating an incorrect value.
    if raw.len() > MAX_TEXT + 8 || raw.contains('\r') || raw.contains("\\`") {
        return None;
    }
    let value = crate::value::string_literal(raw, Language::Python)?;
    (value.len() <= MAX_TEXT).then_some(value)
}
