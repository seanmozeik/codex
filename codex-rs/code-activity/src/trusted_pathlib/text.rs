//! Pure bounded string operations; no filesystem resolution or content reads.

use super::MAX_TEXT;

pub(super) fn path(path: &str) -> String {
    let root = if path.starts_with("//") && !path.starts_with("///") {
        "//"
    } else if path.starts_with('/') {
        "/"
    } else {
        ""
    };
    let parts: Vec<_> = path
        .split('/')
        .filter(|part| !matches!(*part, "" | "."))
        .collect();
    let result = format!("{root}{}", parts.join("/"));
    if result.is_empty() {
        ".".into()
    } else {
        result
    }
}

pub(super) fn replace(receiver: &str, old: &str, new: &str) -> Option<String> {
    let matches = if old.is_empty() {
        receiver.chars().count().checked_add(1)?
    } else {
        receiver.match_indices(old).count()
    };
    let size = receiver
        .len()
        .checked_sub(matches.checked_mul(old.len())?)?
        .checked_add(matches.checked_mul(new.len())?)?;
    if size > MAX_TEXT {
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
    use super::MAX_TEXT;
    use super::replace;
    use pretty_assertions::assert_eq;

    #[test]
    fn exact_python_text_replacement_results() {
        for (input, old, new, expected) in [
            ("aaa", "aa", "b", "ba"),
            ("abc", "z", "x", "abc"),
            ("é🙂", "", "/", "/é/🙂/"),
            ("e\u{301}", "", "-", "-e-\u{301}-"),
            ("", "", "x", "x"),
            ("a", "a", "", ""),
        ] {
            assert_eq!(replace(input, old, new).as_deref(), Some(expected));
        }
    }

    #[test]
    fn replacement_checks_output_bytes_before_allocating() {
        let exact = "x".repeat(MAX_TEXT);
        assert_eq!(replace("a", "a", &exact).as_deref(), Some(exact.as_str()));
        assert_eq!(replace("a", "a", &(exact + "x")), None);
        assert_eq!(replace("aa", "a", &"é".repeat(MAX_TEXT / 4 + 1)), None);
    }
}
