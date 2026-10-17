//! Linear, deterministic endpoint comparisons with one honest enclosing hunk.

use super::SnapshotByteRange;
use super::SnapshotPreviewError;
use super::SnapshotReplacement;
use super::budget::Budget;

pub(super) fn compare(
    before: Option<&str>,
    after: &str,
    budget: &mut Budget,
) -> Result<(Vec<SnapshotReplacement>, String), SnapshotPreviewError> {
    let old = before.unwrap_or_default();
    budget.work(old.len().saturating_add(after.len()).saturating_mul(12))?;
    let replacements = replacement(old, after);
    if before == Some(after) {
        return Ok((replacements, String::new()));
    }
    let mut output = DiffWriter {
        text: String::new(),
        budget,
    };
    output.append(if before.is_some() {
        "--- supplied-before\n"
    } else {
        "--- /dev/null\n"
    })?;
    output.append("+++ conditional-after\n")?;
    let old_count = old.split_inclusive('\n').count();
    let new_count = after.split_inclusive('\n').count();
    if old_count == 0 && new_count == 0 {
        return Ok((replacements, output.text));
    }
    let prefix = old
        .split_inclusive('\n')
        .zip(after.split_inclusive('\n'))
        .take_while(|(left, right)| left == right)
        .count();
    let suffix_limit = old_count.min(new_count).saturating_sub(prefix);
    let suffix = old
        .split_inclusive('\n')
        .rev()
        .zip(after.split_inclusive('\n').rev())
        .take(suffix_limit)
        .take_while(|(left, right)| left == right)
        .count();
    let start = prefix.saturating_sub(3);
    let old_end = (old_count - suffix).saturating_add(3).min(old_count);
    let new_end = (new_count - suffix).saturating_add(3).min(new_count);
    let removed = old_end - start;
    let inserted = new_end - start;
    let old_start = start + usize::from(removed != 0);
    let new_start = start + usize::from(inserted != 0);
    output.append(&format!(
        "@@ -{old_start},{removed} +{new_start},{inserted} @@\n"
    ))?;
    output.lines(' ', old, start, prefix)?;
    output.lines('-', old, prefix, old_count - suffix)?;
    output.lines('+', after, prefix, new_count - suffix)?;
    output.lines(' ', old, old_count - suffix, old_end)?;
    Ok((replacements, output.text))
}

fn replacement(before: &str, after: &str) -> Vec<SnapshotReplacement> {
    if before == after {
        return Vec::new();
    }
    let prefix: usize = before
        .chars()
        .zip(after.chars())
        .take_while(|(a, b)| a == b)
        .map(|(character, _)| character.len_utf8())
        .sum();
    let suffix: usize = before[prefix..]
        .chars()
        .rev()
        .zip(after[prefix..].chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(character, _)| character.len_utf8())
        .sum();
    vec![SnapshotReplacement {
        before: SnapshotByteRange {
            start: prefix,
            end: before.len() - suffix,
        },
        after: SnapshotByteRange {
            start: prefix,
            end: after.len() - suffix,
        },
    }]
}

struct DiffWriter<'a> {
    text: String,
    budget: &'a mut Budget,
}

impl DiffWriter<'_> {
    fn append(&mut self, text: &str) -> Result<(), SnapshotPreviewError> {
        self.budget
            .diff(self.text.len().saturating_add(text.len()))?;
        self.text.push_str(text);
        Ok(())
    }

    fn lines(
        &mut self,
        prefix: char,
        text: &str,
        start: usize,
        end: usize,
    ) -> Result<(), SnapshotPreviewError> {
        for line in text.split_inclusive('\n').skip(start).take(end - start) {
            self.append(prefix.encode_utf8(&mut [0; 4]))?;
            self.append(line)?;
            if !line.ends_with('\n') {
                self.append("\n\\ No newline at end of file\n")?;
            }
        }
        Ok(())
    }
}
