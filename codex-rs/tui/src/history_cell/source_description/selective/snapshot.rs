//! Conditional diffs use the source disclosure, never applied-change history.

use codex_protocol::source_description::SelectiveLaterContext;
use codex_protocol::source_description::SelectiveSnapshotPreview;
use codex_protocol::source_description::SelectiveSnapshotScope;
use ratatui::style::Stylize as _;
use ratatui::text::Line;

const COMPACT_DIFF_LINES: usize = 6;

pub(super) fn compact(
    snapshot: &SelectiveSnapshotPreview,
    later_context: SelectiveLaterContext,
    lines: &mut Vec<Line<'static>>,
) {
    lines.push(
        Line::from(match snapshot.scope {
            SelectiveSnapshotScope::OperationPoint => {
                "  Conditional diff · before execution · operation point"
            }
            SelectiveSnapshotScope::LogicalStream => {
                "  Conditional logical stream diff · before execution"
            }
        })
        .dim(),
    );
    super::logical_stream::snapshot_conditions(snapshot, "  ", lines);
    let uncertainty = match later_context {
        SelectiveLaterContext::Unobserved => "  Final file state is unobserved.",
        SelectiveLaterContext::OpaqueSuffix => {
            "  Final state unknown: later unsupported code may change it."
        }
        SelectiveLaterContext::KnownLaterTouch => {
            "  Final state unknown: a later operation targets this path."
        }
        SelectiveLaterContext::PossibleAliasLaterTouch => {
            "  Final state unknown: a later target may alias this path."
        }
    };
    lines.push(Line::from(uncertainty).dim());
    if snapshot.unified_diff.is_empty() {
        lines.push(Line::from(format!("  {}", unchanged_label(snapshot.scope))).dim());
        return;
    }
    let mut body = snapshot.unified_diff.lines().skip(2);
    lines.extend(body.by_ref().take(COMPACT_DIFF_LINES).map(styled));
    if body.next().is_some() {
        lines.push(Line::from("  … expand for the complete conditional diff").dim());
    }
}

pub(super) fn detail(snapshot: &SelectiveSnapshotPreview, lines: &mut Vec<Line<'static>>) {
    lines.push(
        Line::from(match snapshot.scope {
            SelectiveSnapshotScope::OperationPoint => {
                "Conditional diff from pre-execution snapshots"
            }
            SelectiveSnapshotScope::LogicalStream => {
                "Conditional logical stream diff from a physical seed"
            }
        })
        .bold(),
    );
    super::logical_stream::snapshot_conditions(snapshot, "", lines);
    lines.push(Line::from(match snapshot.scope {
        SelectiveSnapshotScope::OperationPoint => "Assumes paths, existence and contents still match at each operation, except for the earlier edits shown.",
        SelectiveSnapshotScope::LogicalStream => "Assumes the physical seed and captured reads still match after the listed earlier physical operations.",
    }).dim());
    lines.push(
        Line::from("Required operations must finish normally; execution remains unobserved.").dim(),
    );
    lines.push(Line::from(format!(
        "Snapshot target: {:?}",
        snapshot.resolved_target
    )));
    lines.push(
        Line::from(format!(
            "Capture environment {:?} · cwd {:?}",
            snapshot.environment_id, snapshot.resolved_cwd
        ))
        .dim(),
    );
    if !snapshot.replay_ordinals.is_empty() {
        let label = match snapshot.scope {
            SelectiveSnapshotScope::OperationPoint => "Includes",
            SelectiveSnapshotScope::LogicalStream => "Physical seed includes",
        };
        lines.push(
            Line::from(format!(
                "{label} {} modeled earlier operations for this target.",
                snapshot.replay_ordinals.len()
            ))
            .dim(),
        );
    }
    if snapshot.unified_diff.is_empty() {
        lines.push(Line::from(unchanged_label(snapshot.scope)).dim());
    } else {
        lines.extend(snapshot.unified_diff.lines().map(styled));
    }
}

fn unchanged_label(scope: SelectiveSnapshotScope) -> &'static str {
    match scope {
        SelectiveSnapshotScope::OperationPoint => "No byte change at this operation point.",
        SelectiveSnapshotScope::LogicalStream => {
            "No byte change in this conditional logical stream."
        }
    }
}

fn styled(text: &str) -> Line<'static> {
    let line = Line::from(text.to_owned());
    if text.starts_with('+') && !text.starts_with("+++") {
        line.green()
    } else if text.starts_with('-') && !text.starts_with("---") {
        line.red()
    } else if text.starts_with("@@") || text.starts_with("---") || text.starts_with("+++") {
        line.dim()
    } else {
        line
    }
}

#[cfg(test)]
mod tests;
