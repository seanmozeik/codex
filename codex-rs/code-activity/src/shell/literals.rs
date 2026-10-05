//! Shell literals preserve quoting and reject runtime expansions.
use crate::children;
use crate::text;
use tree_sitter::Node;

pub(super) fn literal(node: Node<'_>, source: &str) -> Option<String> {
    let raw = text(node, source);
    match node.kind() {
        "-" => Some("-".into()),
        "concatenation" => children(node)
            .into_iter()
            .map(|part| literal(part, source))
            .collect::<Option<Vec<_>>>()
            .map(|parts| parts.concat()),
        "word" if raw == "\\'" => Some("'".into()),
        "word" if !raw.contains(['\\', '$', '`', '*', '?', '[', '{', '}', '~']) => {
            Some(raw.to_owned())
        }
        "raw_string" => Some(raw[1..raw.len() - 1].to_owned()),
        "string"
            if !children(node).iter().any(|n| {
                matches!(
                    n.kind(),
                    "simple_expansion" | "expansion" | "command_substitution"
                )
            }) =>
        {
            let mut result = String::new();
            let mut chars = raw[1..raw.len() - 1].chars().peekable();
            while let Some(c) = chars.next() {
                if c == '\\'
                    && chars
                        .peek()
                        .is_some_and(|c| matches!(c, '$' | '`' | '"' | '\\' | '\n'))
                {
                    let c = chars.next()?;
                    if c != '\n' {
                        result.push(c);
                    }
                } else {
                    result.push(c);
                }
            }
            Some(result)
        }
        _ => None,
    }
}
