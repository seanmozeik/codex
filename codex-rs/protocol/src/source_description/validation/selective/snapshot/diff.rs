//! Verify one bounded unified hunk against its serialized UTF-8 endpoints.

use crate::source_description::SelectiveSnapshotPreview;

pub(super) fn matches_endpoints(snapshot: &SelectiveSnapshotPreview) -> bool {
    if snapshot.before.as_ref() == Some(&snapshot.after) {
        return snapshot.unified_diff.is_empty();
    }
    let before = snapshot.before.as_deref().unwrap_or_default();
    let mut lines = snapshot.unified_diff.split_inclusive('\n').peekable();
    if lines.next()
        != Some(if snapshot.before.is_some() {
            "--- supplied-before\n"
        } else {
            "--- /dev/null\n"
        })
        || lines.next() != Some("+++ conditional-after\n")
    {
        return false;
    }
    let Some(header) = lines.next() else {
        return before.is_empty() && snapshot.after.is_empty();
    };
    let Some((old_start, old_count, new_start, new_count)) = hunk(header) else {
        return false;
    };
    if old_start != new_start {
        return false;
    }
    let mut old = before.split_inclusive('\n');
    let mut new = snapshot.after.split_inclusive('\n');
    for _ in 0..old_start {
        match (old.next(), new.next()) {
            (Some(left), Some(right)) if left == right => {}
            _ => return false,
        }
    }
    let (mut removed, mut inserted) = (0usize, 0usize);
    while let Some(line) = lines.next() {
        let Some(prefix) = line.bytes().next() else {
            return false;
        };
        if !matches!(prefix, b' ' | b'-' | b'+') {
            return false;
        }
        let mut content = &line[1..];
        if lines.peek() == Some(&"\\ No newline at end of file\n") {
            let Some(without_added_newline) = content.strip_suffix('\n') else {
                return false;
            };
            content = without_added_newline;
            lines.next();
        }
        if matches!(prefix, b' ' | b'-') {
            if old.next() != Some(content) {
                return false;
            }
            removed += 1;
        }
        if matches!(prefix, b' ' | b'+') {
            if new.next() != Some(content) {
                return false;
            }
            inserted += 1;
        }
    }
    removed == old_count && inserted == new_count && old.eq(new)
}

fn hunk(header: &str) -> Option<(usize, usize, usize, usize)> {
    let header = header.strip_prefix("@@ -")?.strip_suffix(" @@\n")?;
    let (old, new) = header.split_once(" +")?;
    let (old_start, old_count) = range(old)?;
    let (new_start, new_count) = range(new)?;
    Some((old_start, old_count, new_start, new_count))
}

fn range(value: &str) -> Option<(usize, usize)> {
    let (start, count) = value.split_once(',')?;
    if !start.bytes().all(|byte| byte.is_ascii_digit())
        || !count.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let start = start.parse::<usize>().ok()?;
    let count = count.parse::<usize>().ok()?;
    let index = if count == 0 {
        start
    } else {
        start.checked_sub(1)?
    };
    // Each endpoint is separately capped, so no valid line index can exceed it.
    (index <= 65_536 && count <= 65_536).then_some((index, count))
}
