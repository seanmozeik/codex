//! Method chains describe source origins without computing runtime return values.

use super::MAX_DEPTH;
use super::MAX_LITERAL_BYTES;
use super::OpenMode;
use super::Origin;
use super::SourceDescriptionFallback;
use super::SourceReferenceKind;
use super::State;
use super::parts;
use crate::Language;
use tree_sitter::Node;

impl State<'_> {
    pub(super) fn eval(&mut self, node: Node<'_>, depth: usize) -> Option<Origin> {
        if depth > MAX_DEPTH {
            self.failure = SourceDescriptionFallback::SourceLimit;
            return None;
        }
        if self.budget.expired() {
            return None;
        }
        match node.kind() {
            "string" => literal(node, self.source).map(Origin::Literal),
            "identifier" => {
                let name = crate::text(node, self.source);
                if name == "open" {
                    return Some(Origin::OpenName);
                }
                let origin = self.bindings.get(name)?;
                (!matches!(origin, Origin::ClosedContext)).then(|| origin.clone())
            }
            "parenthesized_expression" => {
                let items = parts(node);
                let [inner] = items.as_slice() else {
                    return None;
                };
                self.eval(*inner, depth + 1)
            }
            "call" => self.call(node, depth + 1),
            "binary_operator" => self.concatenate(node, depth + 1),
            _ => None,
        }
    }

    fn call(&mut self, node: Node<'_>, depth: usize) -> Option<Origin> {
        if depth > MAX_DEPTH {
            self.failure = SourceDescriptionFallback::SourceLimit;
            return None;
        }
        let callee = node.child_by_field_name("function")?;
        let arguments = parts(node.child_by_field_name("arguments")?);
        if callee.kind() == "identifier" {
            return self.named_call(node, callee, &arguments, depth);
        }
        if callee.kind() != "attribute" {
            return None;
        }
        let receiver = self.eval(callee.child_by_field_name("object")?, depth + 1)?;
        let method = crate::text(callee.child_by_field_name("attribute")?, self.source);
        match (receiver, method) {
            (Origin::PathCall(path), "read_text") => {
                let arguments = encoding_arguments(&arguments, self.source)?;
                if !arguments.is_empty() {
                    return None;
                }
                self.read(node, path)
            }
            (
                Origin::OpenCall {
                    literal: path,
                    mode: OpenMode::Read,
                }
                | Origin::ContextOpen {
                    literal: path,
                    mode: OpenMode::Read,
                },
                "read",
            ) if arguments.is_empty() => self.read(node, path),
            (Origin::PathCall(path), "write_text") => {
                let arguments = encoding_arguments(&arguments, self.source)?;
                self.write(node, &arguments, path, depth)
            }
            (
                Origin::OpenCall {
                    literal: path,
                    mode: OpenMode::Write,
                }
                | Origin::ContextOpen {
                    literal: path,
                    mode: OpenMode::Write,
                },
                "write",
            ) => self.write(node, &arguments, path, depth),
            (Origin::Literal(_), "replace") => self.replacement(node, &arguments, None, depth),
            (Origin::ReadCall(path), "replace") => {
                self.replacement(node, &arguments, Some(path), depth)
            }
            (Origin::ReplacedText(path) | Origin::ComposedText(path), "replace") => {
                self.replacement(node, &arguments, path, depth)
            }
            _ => None,
        }
    }

    fn concatenate(&mut self, node: Node<'_>, depth: usize) -> Option<Origin> {
        if crate::text(node.child_by_field_name("operator")?, self.source) != "+" {
            return None;
        }
        let left = self.eval(node.child_by_field_name("left")?, depth + 1)?;
        let right = self.eval(node.child_by_field_name("right")?, depth + 1)?;
        if !left.is_data() || !right.is_data() {
            return None;
        }
        let path = match (&left, &right) {
            (Origin::Literal(_), other) | (other, Origin::Literal(_)) => {
                other.data_path().map(str::to_owned)
            }
            _ => match (left.data_path(), right.data_path()) {
                (Some(left), Some(right)) if left == right => Some(left.to_owned()),
                _ => None,
            },
        };
        self.retain(path.as_ref().map_or(128, |path| path.len() + 128))?;
        // Even literal + literal is data syntax, not a newly asserted path
        // literal. Mixed/unknown provenance remains absorbing in later chains.
        Some(Origin::ComposedText(path))
    }

    fn named_call(
        &mut self,
        node: Node<'_>,
        callee: Node<'_>,
        arguments: &[Node<'_>],
        depth: usize,
    ) -> Option<Origin> {
        let name = crate::text(callee, self.source);
        if self.constructor.as_deref() == Some(name) {
            let [argument] = arguments else {
                return None;
            };
            let Origin::Literal(path) = self.eval(*argument, depth + 1)? else {
                return None;
            };
            if path.contains('\0') {
                return None;
            }
            self.emit(node, SourceReferenceKind::PathConstructorCall, Some(&path))?;
            return Some(Origin::PathCall(path));
        }
        if name == "print" {
            if arguments.is_empty() {
                return None;
            }
            for argument in arguments {
                if !self.eval(*argument, depth + 1)?.is_data() {
                    return None;
                }
            }
            self.emit(node, SourceReferenceKind::OutputConsumer, None)?;
            return Some(Origin::DiscardedOutput);
        }
        if !self.is_open_name(name) {
            return None;
        }
        let arguments = encoding_arguments(arguments, self.source)?;
        let (path, mode) = match arguments.as_slice() {
            [path] => (*path, OpenMode::Read),
            [path, mode] => {
                let Origin::Literal(mode) = self.eval(*mode, depth + 1)? else {
                    return None;
                };
                let mode = match mode.as_str() {
                    "r" => OpenMode::Read,
                    "w" => OpenMode::Write,
                    _ => return None,
                };
                (*path, mode)
            }
            _ => return None,
        };
        let Origin::Literal(path) = self.eval(path, depth + 1)? else {
            return None;
        };
        if path.contains('\0') {
            return None;
        }
        self.emit(node, SourceReferenceKind::OpenCall, Some(&path))?;
        Some(Origin::OpenCall {
            literal: path,
            mode,
        })
    }

    fn read(&mut self, node: Node<'_>, path: String) -> Option<Origin> {
        self.emit(node, SourceReferenceKind::ReadCall, Some(&path))?;
        Some(Origin::ReadCall(path))
    }

    fn write(
        &mut self,
        node: Node<'_>,
        arguments: &[Node<'_>],
        path: String,
        depth: usize,
    ) -> Option<Origin> {
        let [argument] = arguments else {
            return None;
        };
        if !self.eval(*argument, depth + 1)?.is_data() {
            return None;
        }
        self.emit(node, SourceReferenceKind::WriteCall, Some(&path))?;
        Some(Origin::DiscardedWrite)
    }

    fn replacement(
        &mut self,
        node: Node<'_>,
        arguments: &[Node<'_>],
        path: Option<String>,
        depth: usize,
    ) -> Option<Origin> {
        let (old, new) = match arguments {
            [old, new] => (*old, *new),
            [old, new, count] if decimal_count(*count, self.source) => (*old, *new),
            _ => return None,
        };
        let Origin::Literal(_) = self.eval(old, depth + 1)? else {
            return None;
        };
        let Origin::Literal(_) = self.eval(new, depth + 1)? else {
            return None;
        };
        self.emit(node, SourceReferenceKind::ReplaceCall, path.as_deref())?;
        Some(Origin::ReplacedText(path))
    }
}

fn decimal_count(node: Node<'_>, source: &str) -> bool {
    let token = crate::text(node, source);
    node.kind() == "integer"
        && !token.is_empty()
        && token.bytes().all(|byte| byte.is_ascii_digit())
        && (token == "0" || !token.starts_with('0'))
}

fn literal(node: Node<'_>, source: &str) -> Option<String> {
    let raw = crate::text(node, source);
    // Preserve only forms whose decoded text agrees with Python's lexical rules.
    // Raw literals retain backslashes; f/byte strings are rejected by the decoder.
    if raw.len() > MAX_LITERAL_BYTES + 8 || raw.contains('\r') || raw.contains("\\`") {
        return None;
    }
    let text = crate::value::string_literal(raw, Language::Python)?;
    (text.len() <= MAX_LITERAL_BYTES).then_some(text)
}

fn encoding_arguments<'tree>(arguments: &[Node<'tree>], source: &str) -> Option<Vec<Node<'tree>>> {
    let mut positional = Vec::new();
    let mut encoding_seen = false;
    for argument in arguments {
        if argument.kind() != "keyword_argument" {
            if encoding_seen {
                return None;
            }
            positional.push(*argument);
            continue;
        }
        if encoding_seen {
            return None;
        }
        let name = argument.child_by_field_name("name")?;
        let value = argument.child_by_field_name("value")?;
        if name.kind() != "identifier"
            || crate::text(name, source) != "encoding"
            || value.kind() != "string"
            || !matches!(literal(value, source)?.as_str(), "utf-8" | "utf8")
        {
            return None;
        }
        encoding_seen = true;
    }
    Some(positional)
}
