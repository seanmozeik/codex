//! Strict Python literal string folding, without extending content or import trust.

use super::Mode;
use super::State;
use crate::Language;
use crate::value::MAX_VALUE_BYTES;
use crate::value::Value;
use std::collections::BTreeMap;
use tree_sitter::Node;

// Value::budget_size charges 64 bytes for the text value in addition to its payload.
const MAX_TEXT_BYTES: usize = MAX_VALUE_BYTES - 64;

pub(super) fn evaluate(
    state: &mut State<'_, '_>,
    node: Node<'_>,
    receiver: &Value,
    method: &str,
    args: &[Value],
    keywords: &BTreeMap<String, Value>,
) -> Option<Value> {
    if state.mode != Mode::Complete || state.language != Language::Python || method != "replace" {
        return None;
    }
    let Value::Text(receiver) = receiver else {
        return None;
    };
    let unsupported = "Literal Python replacement requires two ordinary text arguments";
    if !keywords.is_empty() || !ordinary_arguments(node) {
        state.gap(node, unsupported);
        return Some(Value::Unknown);
    }
    let [Value::Text(old), Value::Text(new)] = args else {
        state.gap(node, unsupported);
        return Some(Value::Unknown);
    };
    Some(match fold(receiver, old, new) {
        Some(result) => Value::Text(result),
        None => {
            state.gap(
                node,
                "Literal Python replacement exceeds the abstract value budget",
            );
            Value::Unknown
        }
    })
}

fn ordinary_arguments(node: Node<'_>) -> bool {
    let Some(arguments) = node.child_by_field_name("arguments") else {
        return false;
    };
    let mut cursor = arguments.walk();
    let mut count = 0;
    for argument in arguments.named_children(&mut cursor) {
        match argument.kind() {
            "comment" => continue,
            "keyword_argument" | "list_splat" | "dictionary_splat" => return false,
            _ => count += 1,
        }
    }
    count == 2
}

fn fold(receiver: &str, old: &str, new: &str) -> Option<String> {
    let matches = if old.is_empty() {
        receiver.chars().count().checked_add(1)?
    } else {
        receiver.match_indices(old).count()
    };
    let size = receiver
        .len()
        .checked_sub(matches.checked_mul(old.len())?)?
        .checked_add(matches.checked_mul(new.len())?)?;
    if size > MAX_TEXT_BYTES {
        return None;
    }
    let mut output = String::with_capacity(size);
    if old.is_empty() {
        output.push_str(new);
        for character in receiver.chars() {
            output.push(character);
            output.push_str(new);
        }
    } else {
        let mut start = 0;
        for (offset, matched) in receiver.match_indices(old) {
            output.push_str(&receiver[start..offset]);
            output.push_str(new);
            start = offset + matched.len();
        }
        output.push_str(&receiver[start..]);
    }
    Some(output)
}

#[cfg(test)]
mod tests {
    use super::MAX_TEXT_BYTES;
    use super::fold;
    use pretty_assertions::assert_eq;

    #[test]
    fn replacement_is_nonoverlapping_and_preserves_unicode_boundaries() {
        for (input, old, new, expected) in [
            ("aaa", "aa", "b", "ba"),
            ("a.old.old", ".old", ".new", "a.new.new"),
            ("abc", "z", "x", "abc"),
            ("é🙂", "", "/", "/é/🙂/"),
            ("e\u{301}", "", "-", "-e-\u{301}-"),
            ("", "", "x", "x"),
            ("a", "a", "", ""),
        ] {
            assert_eq!(fold(input, old, new).as_deref(), Some(expected));
        }
    }

    #[test]
    fn output_budget_counts_utf8_bytes_before_allocation() {
        let exact = "x".repeat(MAX_TEXT_BYTES);
        assert_eq!(fold("a", "a", &exact).as_deref(), Some(exact.as_str()));
        assert_eq!(fold("a", "a", &(exact + "x")), None);
        let multibyte = "é".repeat(MAX_TEXT_BYTES / 2);
        assert_eq!(
            fold("a", "a", &multibyte).as_deref(),
            Some(multibyte.as_str())
        );
        assert_eq!(fold("a", "", &"x".repeat(MAX_TEXT_BYTES / 2)), None);
    }
}
