//! Closed calls retain source provenance without predicting returned bytes.

use super::MAX_DEPTH;
use super::MAX_LITERAL_BYTES;
use super::Origin;
use super::SourceDescriptionFallback;
use super::SourceReferenceKind;
use super::State;
use super::call_arguments;
use super::literal;
use super::module_literal;
use super::parts;
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
            "identifier" => self
                .bindings
                .get(crate::text(node, self.source))
                .map(|binding| binding.origin.clone()),
            "parenthesized_expression" => {
                let nodes = parts(node);
                let [expression] = nodes.as_slice() else {
                    return None;
                };
                self.eval(*expression, depth + 1)
            }
            "call_expression" => self.call(node, depth + 1),
            _ => None,
        }
    }

    fn call(&mut self, node: Node<'_>, depth: usize) -> Option<Origin> {
        if depth > MAX_DEPTH {
            self.failure = SourceDescriptionFallback::SourceLimit;
            return None;
        }
        let arguments = call_arguments(node, self.source)?;
        let function = node.child_by_field_name("function")?;
        if function.kind() == "identifier" && crate::text(function, self.source) == "print" {
            return self.output(node, &arguments, depth);
        }
        if parts(function).len() != 2 {
            return None;
        }
        let object = function.child_by_field_name("object")?;
        let method = self.member_name(function, depth)?;
        if object.kind() == "identifier"
            && self.module.as_deref() == Some(crate::text(object, self.source))
        {
            return self.file_call(node, &method, &arguments, depth);
        }
        if object.kind() == "call_expression" {
            let callee = object.child_by_field_name("function")?;
            if callee.kind() == "identifier" && crate::text(callee, self.source) == "require" {
                let require_args = call_arguments(object, self.source)?;
                let [argument] = require_args.as_slice() else {
                    return None;
                };
                let module = module_literal(*argument, self.source)?;
                self.emit(object, SourceReferenceKind::ImportFrom, Some(&module))?;
                return self.file_call(node, &method, &arguments, depth);
            }
        }
        // Computed keys are supported only on the recognized filesystem origin.
        if function.kind() != "member_expression" {
            return None;
        }
        if object.kind() == "identifier"
            && crate::text(object, self.source) == "console"
            && method == "log"
        {
            return self.output(node, &arguments, depth);
        }
        let receiver = self.eval(object, depth + 1)?;
        if !matches!(method.as_str(), "replace" | "replaceAll") {
            return None;
        }
        let [old, new] = arguments.as_slice() else {
            return None;
        };
        // Only literal strings, never RegExp, callbacks, spread or coercion.
        literal(*old, self.source)?;
        literal(*new, self.source)?;
        let path = match receiver {
            Origin::Literal(_) => None,
            Origin::Read(path) => Some(path),
            Origin::Replaced(path) => path,
            Origin::Discarded => return None,
        };
        self.emit(node, SourceReferenceKind::ReplaceCall, path.as_deref())?;
        Some(Origin::Replaced(path))
    }

    fn member_name(&mut self, node: Node<'_>, depth: usize) -> Option<String> {
        match node.kind() {
            "member_expression" => {
                let property = node.child_by_field_name("property")?;
                (property.kind() == "property_identifier")
                    .then(|| crate::text(property, self.source).to_owned())
            }
            "subscript_expression" => {
                self.member_literal(node.child_by_field_name("index")?, depth + 1)
            }
            _ => None,
        }
    }

    fn member_literal(&mut self, node: Node<'_>, depth: usize) -> Option<String> {
        if depth > MAX_DEPTH {
            self.failure = SourceDescriptionFallback::SourceLimit;
            return None;
        }
        if self.budget.expired() {
            return None;
        }
        match node.kind() {
            "string" => literal(node, self.source),
            "parenthesized_expression" => {
                let nodes = parts(node);
                let [expression] = nodes.as_slice() else {
                    return None;
                };
                self.member_literal(*expression, depth + 1)
            }
            "binary_expression" => {
                if parts(node).len() != 2
                    || crate::text(node.child_by_field_name("operator")?, self.source) != "+"
                {
                    return None;
                }
                let mut left = self.member_literal(node.child_by_field_name("left")?, depth + 1)?;
                let right = self.member_literal(node.child_by_field_name("right")?, depth + 1)?;
                if left.len().checked_add(right.len())? > MAX_LITERAL_BYTES {
                    self.failure = SourceDescriptionFallback::SourceLimit;
                    return None;
                }
                left.push_str(&right);
                Some(left)
            }
            _ => None,
        }
    }

    fn file_call(
        &mut self,
        node: Node<'_>,
        method: &str,
        arguments: &[Node<'_>],
        depth: usize,
    ) -> Option<Origin> {
        match (method, arguments) {
            ("readFileSync", [path, encoding]) => {
                encoding_literal(*encoding, self.source)?;
                let path = self.path(*path, depth)?;
                self.emit(node, SourceReferenceKind::ReadCall, Some(&path))?;
                Some(Origin::Read(path))
            }
            ("writeFileSync", [path, data] | [path, data, _]) => {
                if let [_, _, encoding] = arguments {
                    encoding_literal(*encoding, self.source)?;
                }
                let path = self.path(*path, depth)?;
                if !self.eval(*data, depth + 1)?.is_data() {
                    return None;
                }
                self.emit(node, SourceReferenceKind::WriteCall, Some(&path))?;
                Some(Origin::Discarded)
            }
            _ => None,
        }
    }

    fn path(&mut self, node: Node<'_>, depth: usize) -> Option<String> {
        let Origin::Literal(path) = self.eval(node, depth + 1)? else {
            return None;
        };
        (!path.contains('\0')).then_some(path)
    }

    fn output(&mut self, node: Node<'_>, arguments: &[Node<'_>], depth: usize) -> Option<Origin> {
        if arguments.is_empty() {
            return None;
        }
        for argument in arguments {
            if !self.eval(*argument, depth + 1)?.is_data() {
                return None;
            }
        }
        self.emit(node, SourceReferenceKind::OutputConsumer, None)?;
        Some(Origin::Discarded)
    }
}

fn encoding_literal(node: Node<'_>, source: &str) -> Option<()> {
    (literal(node, source)? == "utf8").then_some(())
}
