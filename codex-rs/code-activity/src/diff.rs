use crate::DiffAttribution;
use crate::FileDiff;
use crate::FileSnapshot;
use similar::TextDiff;

pub fn compare(snapshots: Vec<FileSnapshot>) -> Vec<FileDiff> {
    let omitted_files = snapshots.len().saturating_sub(256);
    let mut remaining_bytes = crate::MAX_SOURCE_BYTES;
    let mut results: Vec<_> = snapshots
        .into_iter()
        .take(256)
        .map(|s| {
            let before = s.before.as_deref().unwrap_or("");
            let after = s.after.as_deref().unwrap_or("");
            let bytes = before.len() + after.len();
            if bytes > 262_144 || bytes > remaining_bytes {
                return FileDiff::Omitted {
                    path: s.path,
                    reason: "Snapshot byte budget exceeded".into(),
                };
            }
            remaining_bytes -= bytes;
            let changed = s.before != s.after;
            let old = if s.before.is_some() {
                s.path.as_str()
            } else {
                "/dev/null"
            };
            let new = if s.after.is_some() {
                s.path.as_str()
            } else {
                "/dev/null"
            };
            let diff = TextDiff::configure()
                .timeout(std::time::Duration::from_millis(25))
                .diff_lines(before, after);
            let unified_diff = diff.unified_diff().header(old, new).to_string();
            FileDiff::Compared {
                path: s.path,
                changed,
                unified_diff,
                attribution: DiffAttribution::HostSnapshotsOnly,
            }
        })
        .collect();
    if omitted_files > 0 {
        results.push(FileDiff::Truncated { omitted_files });
    }
    results
}
