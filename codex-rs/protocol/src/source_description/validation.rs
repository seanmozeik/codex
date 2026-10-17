mod selective;

use super::MAX_PATCH_BYTES;
use super::MAX_PATCHES;
use super::MAX_REFERENCES;
use super::MAX_SOURCE_BYTES;
use super::MAX_SOURCES;
use super::SourceDescription;
use super::SourceDescriptionFallback;
use super::SourceDescriptionReferenceKind;
use super::SourceDescriptionSource;
use super::SourceDescriptionSpan;
use super::patch_validation::PatchBudget;

impl SourceDescription {
    /// Validate the entire bounded presentation without modifying source bytes.
    /// `may_display` applies the host's disclosure policy to each retained string.
    /// A redaction would invalidate evidence, so any rejected string discards all
    /// enrichment. These are logical payload limits, not a transport allocation cap.
    /// This checks evidence structure and display safety, not grammar completeness
    /// or authenticity. The producing analyzer owns literal cooking and syntax;
    /// this DTO is never used for execution, permissions, or observed file changes.
    pub fn into_presentable(self, may_display: impl Fn(&str) -> bool) -> Self {
        if self.is_presentable(&may_display) {
            self
        } else {
            Self::Fallback {
                reason: SourceDescriptionFallback::Presentation,
            }
        }
    }

    fn is_presentable(&self, may_display: &impl Fn(&str) -> bool) -> bool {
        if let Self::SelectiveFindings {
            sources,
            edits,
            barriers,
        } = self
        {
            return selective::is_presentable(sources, edits, barriers, may_display);
        }
        let Self::CompleteSyntax {
            sources,
            references,
            patches,
        } = self
        else {
            return true;
        };
        if sources.is_empty()
            || sources.len() > MAX_SOURCES
            || references.len() > MAX_REFERENCES
            || patches.len() > MAX_PATCHES
        {
            return false;
        }
        let mut decoded_bytes = 0usize;
        for (id, source) in sources.iter().enumerate() {
            decoded_bytes = decoded_bytes.saturating_add(source.text.len());
            if source.id != id
                || decoded_bytes > MAX_SOURCE_BYTES
                || !display_text(&source.text, may_display)
            {
                return false;
            }
            match &source.parent {
                None if id == 0 => {}
                Some(parent)
                    if parent.source_id < id
                        && if source.text.is_empty() {
                            parent.is_valid_or_empty(sources)
                        } else {
                            parent.is_valid(sources)
                        } => {}
                _ => return false,
            }
        }
        let mut literal_bytes = 0usize;
        for reference in references {
            let valid_span = if reference.kind == SourceDescriptionReferenceKind::HeredocData
                && reference.literal.is_none()
            {
                reference.evidence.is_valid_or_empty(sources)
            } else {
                reference.evidence.is_valid(sources)
            };
            if !valid_span {
                return false;
            }
            if let Some(literal) = &reference.literal {
                literal_bytes = literal_bytes.saturating_add(literal.len());
                if literal_bytes > MAX_SOURCE_BYTES || !display_text(literal, may_display) {
                    return false;
                }
            }
        }
        let mut budget = PatchBudget::default();
        for (id, patch) in patches.iter().enumerate() {
            decoded_bytes = decoded_bytes.saturating_add(patch.payload.len());
            if patch.id != id
                || patch.payload.len() > MAX_PATCH_BYTES
                || decoded_bytes > MAX_SOURCE_BYTES
                || !display_text(&patch.payload, may_display)
                || !patch.invocation.is_valid(sources)
                || !patch.argument.is_valid(sources)
                || patch.argument.source_id != patch.invocation.source_id
                || patch.argument.start_byte < patch.invocation.start_byte
                || patch.argument.end_byte > patch.invocation.end_byte
                || !references.iter().any(|reference| {
                    reference.kind == SourceDescriptionReferenceKind::InvocationCall
                        && reference.evidence == patch.invocation
                        && reference.literal.is_none()
                })
                || !patch.is_valid(&mut budget)
            {
                return false;
            }
        }
        true
    }
}

impl SourceDescriptionSpan {
    fn is_valid(&self, sources: &[SourceDescriptionSource]) -> bool {
        self.start_byte < self.end_byte && self.is_valid_or_empty(sources)
    }

    // Empty physical heredoc bodies still have an exact, bounded UTF-8 location.
    // Callers permit this only for literal data evidence or an empty owned source.
    fn is_valid_or_empty(&self, sources: &[SourceDescriptionSource]) -> bool {
        let Some(source) = sources.get(self.source_id) else {
            return false;
        };
        if self.start_byte > self.end_byte
            || source.text.get(self.start_byte..self.end_byte).is_none()
        {
            return false;
        }
        let before = &source.text[..self.start_byte];
        let through_end = &source.text[..self.end_byte];
        self.start_line == before.bytes().filter(|byte| *byte == b'\n').count() + 1
            && self.end_line == through_end.bytes().filter(|byte| *byte == b'\n').count() + 1
            && self.start_column == before.rsplit('\n').next().unwrap_or_default().len() + 1
    }
}

// Refuse text requiring terminal escaping; never silently alter indexed bytes.
fn display_text(text: &str, may_display: &impl Fn(&str) -> bool) -> bool {
    !text.chars().any(|ch| {
        (ch.is_control() && ch != '\n' && ch != '\t')
            || matches!(ch, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
    }) && may_display(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_description::SourceDescriptionLanguage;
    use crate::source_description::SourceDescriptionReference;
    use crate::source_description::SourceDescriptionReferenceKind;
    use pretty_assertions::assert_eq;

    fn description(text: &str) -> SourceDescription {
        SourceDescription::CompleteSyntax {
            sources: vec![SourceDescriptionSource {
                id: 0,
                language: SourceDescriptionLanguage::TypeScript,
                parent: None,
                text: text.to_owned(),
            }],
            references: vec![SourceDescriptionReference {
                kind: SourceDescriptionReferenceKind::InvocationCall,
                evidence: SourceDescriptionSpan {
                    source_id: 0,
                    start_byte: 0,
                    end_byte: text.len(),
                    start_line: 1,
                    end_line: 1,
                    start_column: 1,
                },
                literal: None,
            }],
            patches: vec![],
        }
    }

    #[test]
    fn disclosure_rejection_discards_every_child() {
        let result = description("secret()").into_presentable(|text| !text.contains("secret"));
        assert_eq!(
            serde_json::to_value(result).expect("serialize fallback"),
            serde_json::json!({"disposition":"fallback", "reason":"presentation"})
        );
    }

    #[test]
    fn invalid_utf8_boundary_discards_the_complete_description() {
        let mut input = description("é()");
        let SourceDescription::CompleteSyntax { references, .. } = &mut input else {
            unreachable!();
        };
        references[0].evidence.start_byte = 1;
        assert_eq!(
            input.into_presentable(|_| true),
            SourceDescription::Fallback {
                reason: SourceDescriptionFallback::Presentation,
            }
        );
    }

    #[test]
    fn unknown_fallback_child_fields_are_rejected() {
        assert!(
            serde_json::from_value::<SourceDescription>(serde_json::json!({
                "disposition":"fallback", "reason":"unsupportedSource", "patches":[]
            }))
            .is_err()
        );
    }

    fn empty_heredoc_description() -> SourceDescription {
        let body = SourceDescriptionSpan {
            source_id: 0,
            start_byte: 12,
            end_byte: 12,
            start_line: 2,
            end_line: 2,
            start_column: 1,
        };
        SourceDescription::CompleteSyntax {
            sources: vec![SourceDescriptionSource {
                id: 0,
                language: SourceDescriptionLanguage::Shell,
                parent: None,
                text: "cat <<'EOF'\nEOF\n".to_owned(),
            }],
            references: vec![SourceDescriptionReference {
                kind: SourceDescriptionReferenceKind::HeredocData,
                evidence: body,
                literal: None,
            }],
            patches: vec![],
        }
    }

    fn empty_owned_heredoc_description() -> SourceDescription {
        SourceDescription::CompleteSyntax {
            sources: vec![
                SourceDescriptionSource {
                    id: 0,
                    language: SourceDescriptionLanguage::Shell,
                    parent: None,
                    text: "python - <<'EOF'\nEOF\n".to_owned(),
                },
                SourceDescriptionSource {
                    id: 1,
                    language: SourceDescriptionLanguage::Python,
                    parent: Some(SourceDescriptionSpan {
                        source_id: 0,
                        start_byte: 17,
                        end_byte: 17,
                        start_line: 2,
                        end_line: 2,
                        start_column: 1,
                    }),
                    text: String::new(),
                },
            ],
            references: vec![],
            patches: vec![],
        }
    }

    #[test]
    fn source_empty_heredoc_locations_survive_presentation() {
        for input in [
            empty_heredoc_description(),
            empty_owned_heredoc_description(),
        ] {
            assert_eq!(input.clone().into_presentable(|_| true), input);
        }
    }

    #[test]
    fn source_empty_span_exception_does_not_admit_ordinary_references() {
        let mut input = empty_heredoc_description();
        let SourceDescription::CompleteSyntax { references, .. } = &mut input else {
            unreachable!();
        };
        references[0].kind = SourceDescriptionReferenceKind::InvocationCall;
        assert_eq!(
            input.into_presentable(|_| true),
            SourceDescription::Fallback {
                reason: SourceDescriptionFallback::Presentation,
            },
        );
    }

    #[test]
    fn source_empty_span_exception_requires_empty_child_and_literal_absence() {
        for change_child in [true, false] {
            let mut input = if change_child {
                empty_owned_heredoc_description()
            } else {
                empty_heredoc_description()
            };
            let SourceDescription::CompleteSyntax {
                sources,
                references,
                ..
            } = &mut input
            else {
                unreachable!();
            };
            if change_child {
                sources[1].text = "pass\n".to_owned();
            } else {
                references[0].literal = Some(String::new());
            }
            assert_eq!(
                input.into_presentable(|_| true),
                SourceDescription::Fallback {
                    reason: SourceDescriptionFallback::Presentation,
                },
            );
        }
    }

    #[test]
    fn source_empty_heredoc_still_requires_exact_bounded_location() {
        for invalid_location in [0, 1, 2] {
            let mut input = empty_heredoc_description();
            let SourceDescription::CompleteSyntax { references, .. } = &mut input else {
                unreachable!();
            };
            let span = &mut references[0].evidence;
            match invalid_location {
                0 => span.start_line = 1,
                1 => span.start_column = 2,
                _ => {
                    span.start_byte = 17;
                    span.end_byte = 17;
                }
            }
            assert_eq!(
                input.into_presentable(|_| true),
                SourceDescription::Fallback {
                    reason: SourceDescriptionFallback::Presentation,
                },
            );
        }
    }
}
