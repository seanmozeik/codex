//! Source-only parent cell shared by live completion and restored history.

mod selective;

use super::ActivityDisclosure;
use super::HistoryCell;
use super::PrefixedWrappedHistoryCell;
use codex_app_server_protocol::source_description_for_display;
use codex_protocol::source_description::SourceDescription;
use codex_protocol::source_description::SourceDescriptionReference;
use codex_protocol::source_description::SourceDescriptionReferenceKind;
use codex_protocol::source_description::SubmittedPatch;
use ratatui::style::Stylize as _;
use ratatui::text::Line;
use ratatui::text::Text;

#[derive(Debug)]
pub(crate) struct SourceDescriptionCell {
    id: String,
    references: Vec<SourceDescriptionReference>,
    patches: Vec<SubmittedPatch>,
    selective: Option<selective::SelectiveCell>,
}

impl SourceDescriptionCell {
    pub(crate) fn new(id: String, description: SourceDescription) -> Option<Self> {
        let (references, patches) = match source_description_for_display(description) {
            SourceDescription::CompleteSyntax {
                references,
                patches,
                ..
            } => (references, patches),
            SourceDescription::SelectiveFindings {
                edits, barriers, ..
            } => {
                return Some(Self {
                    id,
                    references: vec![],
                    patches: vec![],
                    selective: Some(selective::SelectiveCell { edits, barriers }),
                });
            }
            SourceDescription::Fallback { .. } => return None,
        };
        if references.is_empty() && patches.is_empty() {
            return None;
        }
        Some(Self {
            id,
            references,
            patches,
            selective: None,
        })
    }

    fn summary(&self) -> String {
        if let Some(selective) = &self.selective {
            return selective.summary();
        }
        format!(
            "Source · {} references · {} submitted patches",
            self.references.len(),
            self.patches.len()
        )
    }

    fn detail(&self) -> Vec<Line<'static>> {
        if let Some(selective) = &self.selective {
            return selective.detail();
        }
        let mut lines = vec![Line::from(self.summary()).dim()];
        for reference in &self.references {
            let evidence = &reference.evidence;
            lines.push(
                Line::from(format!(
                    "{} · source {}:{}:{}",
                    reference_label(reference.kind),
                    evidence.source_id,
                    evidence.start_line,
                    evidence.start_column,
                ))
                .dim(),
            );
            if let Some(literal) = &reference.literal {
                lines.extend(literal.split('\n').map(|line| Line::from(line.to_owned())));
            }
        }
        for patch in &self.patches {
            lines.push(Line::default());
            lines
                .push(Line::from(format!("Submitted patch {} · source text", patch.id + 1)).bold());
            // Preserve the cooked source ordering. No file hyperlinks, reconstructed
            // before/after diff, or applied/success badge is derived from this text.
            lines.extend(
                patch
                    .payload
                    .split_terminator('\n')
                    .map(|line| Line::from(line.to_owned())),
            );
        }
        lines
    }
}

impl HistoryCell for SourceDescriptionCell {
    fn activity_ids(&self) -> Vec<String> {
        vec![format!("source:{}", self.id)]
    }

    fn activity_disclosure(&self, _width: u16) -> Option<ActivityDisclosure> {
        Some(ActivityDisclosure::Generic)
    }

    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        if let Some(selective) = &self.selective {
            return PrefixedWrappedHistoryCell::new(
                Text::from(selective.compact()),
                "• ".dim(),
                "  ",
            )
            .display_lines(width);
        }
        PrefixedWrappedHistoryCell::new(self.summary().dim(), "• ".dim(), "  ").display_lines(width)
    }

    fn transcript_lines(&self, width: u16) -> Vec<Line<'static>> {
        PrefixedWrappedHistoryCell::new(Text::from(self.detail()), "  ", "  ").display_lines(width)
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        super::plain_lines(self.detail())
    }
}

fn reference_label(kind: SourceDescriptionReferenceKind) -> &'static str {
    match kind {
        SourceDescriptionReferenceKind::ImportFrom => "Import spelling",
        SourceDescriptionReferenceKind::PathConstructorCall => "Path constructor spelling",
        SourceDescriptionReferenceKind::OpenCall => "Open call spelling",
        SourceDescriptionReferenceKind::ReadCall => "Read call spelling",
        SourceDescriptionReferenceKind::ReplaceCall => "Replace call spelling",
        SourceDescriptionReferenceKind::TransformCall => "Transform spelling",
        SourceDescriptionReferenceKind::WriteCall => "Write call spelling",
        SourceDescriptionReferenceKind::InvocationCall => "Invocation spelling",
        SourceDescriptionReferenceKind::CommandOption => "Command option",
        SourceDescriptionReferenceKind::OutputConsumer => "Output consumer spelling",
        SourceDescriptionReferenceKind::ShellConnector => "Shell connector spelling",
        SourceDescriptionReferenceKind::ShellRedirection => "Shell redirection operand",
        SourceDescriptionReferenceKind::DirectoryOperand => "Directory operand",
        SourceDescriptionReferenceKind::EnvironmentAssignment => "Environment assignment spelling",
        SourceDescriptionReferenceKind::HeredocData => "Literal heredoc data",
    }
}

#[cfg(test)]
mod tests;
