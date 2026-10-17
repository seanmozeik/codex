mod logical;
mod selective;
mod snapshot;

use codex_app_server_protocol::source_description_for_display;
use codex_code_activity as activity;
use codex_protocol::source_description as wire;

pub(super) fn forecast(
    result: &activity::SelectiveEditAnalysis,
    snapshots: &[super::supplier::AutomaticSnapshot],
) -> wire::SourceDescription {
    let mut description = selective::description(result);
    for snapshot in snapshots {
        if !snapshot::matches_analysis(snapshot, result) {
            continue;
        }
        let Some(value) = snapshot::convert(snapshot) else {
            continue;
        };
        let mut candidate = description.clone();
        let wire::SourceDescription::SelectiveFindings { edits, .. } = &mut candidate else {
            break;
        };
        let Some(edit) = edits.get_mut(snapshot.preview.ordinal) else {
            continue;
        };
        edit.snapshot = Some(value);
        let candidate = source_description_for_display(candidate);
        if matches!(candidate, wire::SourceDescription::SelectiveFindings { .. }) {
            description = candidate;
        }
    }
    description
}

pub(super) fn description(result: &activity::GuardedSourceDescription) -> wire::SourceDescription {
    if let Some(description) = result.complete_description() {
        return source_description_for_display(complete(description).unwrap_or(
            wire::SourceDescription::Fallback {
                reason: wire::SourceDescriptionFallback::UnsupportedSource,
            },
        ));
    }
    let reason = match result.disposition() {
        activity::SourceDescriptionGuardDisposition::Fallback(activity::GuardFallback::Busy) => {
            wire::SourceDescriptionFallback::Busy
        }
        activity::SourceDescriptionGuardDisposition::Fallback(
            activity::GuardFallback::Deadline,
        ) => wire::SourceDescriptionFallback::Deadline,
        activity::SourceDescriptionGuardDisposition::Fallback(
            activity::GuardFallback::RequestTooLarge,
        ) => wire::SourceDescriptionFallback::SourceLimit,
        activity::SourceDescriptionGuardDisposition::Fallback(
            activity::GuardFallback::Description(reason),
        ) => match reason {
            activity::SourceDescriptionFallback::SourceLimit => {
                wire::SourceDescriptionFallback::SourceLimit
            }
            activity::SourceDescriptionFallback::TimeBudget => {
                wire::SourceDescriptionFallback::Deadline
            }
            _ => wire::SourceDescriptionFallback::UnsupportedSource,
        },
        _ => wire::SourceDescriptionFallback::Unavailable,
    };
    wire::SourceDescription::Fallback { reason }
}

struct Unsupported;

fn complete(
    description: &activity::SourceDescription,
) -> Result<wire::SourceDescription, Unsupported> {
    if description.sources().len() != description.source_texts().len() {
        return Err(Unsupported);
    }
    let sources = description
        .sources()
        .iter()
        .zip(description.source_texts())
        .map(|(source, text)| {
            Ok(wire::SourceDescriptionSource {
                id: source.id.index(),
                language: match source.language {
                    activity::Language::TypeScript => wire::SourceDescriptionLanguage::TypeScript,
                    activity::Language::Shell => wire::SourceDescriptionLanguage::Shell,
                    activity::Language::Python => wire::SourceDescriptionLanguage::Python,
                    _ => return Err(Unsupported),
                },
                parent: source.parent.as_ref().map(span),
                text: text.clone(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let references = description
        .references()
        .iter()
        .map(|reference| {
            Ok(wire::SourceDescriptionReference {
                kind: match reference.kind {
                    activity::SourceReferenceKind::ImportFrom => {
                        wire::SourceDescriptionReferenceKind::ImportFrom
                    }
                    activity::SourceReferenceKind::PathConstructorCall => {
                        wire::SourceDescriptionReferenceKind::PathConstructorCall
                    }
                    activity::SourceReferenceKind::OpenCall => {
                        wire::SourceDescriptionReferenceKind::OpenCall
                    }
                    activity::SourceReferenceKind::ReadCall => {
                        wire::SourceDescriptionReferenceKind::ReadCall
                    }
                    activity::SourceReferenceKind::ReplaceCall => {
                        wire::SourceDescriptionReferenceKind::ReplaceCall
                    }
                    activity::SourceReferenceKind::TransformCall => {
                        wire::SourceDescriptionReferenceKind::TransformCall
                    }
                    activity::SourceReferenceKind::WriteCall => {
                        wire::SourceDescriptionReferenceKind::WriteCall
                    }
                    activity::SourceReferenceKind::InvocationCall => {
                        wire::SourceDescriptionReferenceKind::InvocationCall
                    }
                    activity::SourceReferenceKind::CommandOption => {
                        wire::SourceDescriptionReferenceKind::CommandOption
                    }
                    activity::SourceReferenceKind::OutputConsumer => {
                        wire::SourceDescriptionReferenceKind::OutputConsumer
                    }
                    activity::SourceReferenceKind::ShellConnector => {
                        wire::SourceDescriptionReferenceKind::ShellConnector
                    }
                    activity::SourceReferenceKind::ShellRedirection => {
                        wire::SourceDescriptionReferenceKind::ShellRedirection
                    }
                    activity::SourceReferenceKind::DirectoryOperand => {
                        wire::SourceDescriptionReferenceKind::DirectoryOperand
                    }
                    activity::SourceReferenceKind::EnvironmentAssignment => {
                        wire::SourceDescriptionReferenceKind::EnvironmentAssignment
                    }
                    activity::SourceReferenceKind::HeredocData => {
                        wire::SourceDescriptionReferenceKind::HeredocData
                    }
                    _ => return Err(Unsupported),
                },
                evidence: span(&reference.evidence),
                literal: reference.literal.clone(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let patches = description
        .patches()
        .iter()
        .map(|patch| {
            Ok(wire::SubmittedPatch {
                id: patch.id.index(),
                invocation: span(&patch.invocation),
                argument: span(&patch.argument),
                payload: patch.payload.clone(),
                files: patch
                    .files
                    .iter()
                    .map(file)
                    .collect::<Result<Vec<_>, _>>()?,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(wire::SourceDescription::CompleteSyntax {
        sources,
        references,
        patches,
    })
}

fn span(value: &activity::Span) -> wire::SourceDescriptionSpan {
    wire::SourceDescriptionSpan {
        source_id: value.source_id.index(),
        start_byte: value.start_byte,
        end_byte: value.end_byte,
        start_line: value.start_line,
        end_line: value.end_line,
        start_column: value.start_column,
    }
}

fn evidence(value: &activity::PatchEvidence) -> wire::SubmittedPatchEvidence {
    wire::SubmittedPatchEvidence {
        patch_id: value.patch_id.index(),
        range: wire::SubmittedPatchRange {
            start: value.range.start,
            end: value.range.end,
        },
    }
}

fn path(value: &activity::PatchPathReference) -> wire::SubmittedPatchPath {
    wire::SubmittedPatchPath {
        spelling: value.spelling.as_str().to_owned(),
        evidence: evidence(&value.evidence),
    }
}

fn line(value: &activity::PatchLine) -> wire::SubmittedPatchLine {
    wire::SubmittedPatchLine {
        prefix: match value.prefix {
            activity::PatchLinePrefix::Context => wire::SubmittedPatchLinePrefix::Context,
            activity::PatchLinePrefix::Plus => wire::SubmittedPatchLinePrefix::Plus,
            activity::PatchLinePrefix::Minus => wire::SubmittedPatchLinePrefix::Minus,
        },
        evidence: evidence(&value.evidence),
        text: evidence(&value.text),
    }
}

fn file(value: &activity::SourcePatchFile) -> Result<wire::SubmittedPatchFile, Unsupported> {
    Ok(match value {
        activity::SourcePatchFile::Add { path: value, lines } => wire::SubmittedPatchFile::Add {
            path: path(value),
            lines: lines.iter().map(line).collect(),
        },
        activity::SourcePatchFile::Delete { path: value } => {
            wire::SubmittedPatchFile::Delete { path: path(value) }
        }
        activity::SourcePatchFile::Update {
            path: value,
            move_to,
            chunks,
        } => wire::SubmittedPatchFile::Update {
            path: path(value),
            move_to: move_to.as_ref().map(path),
            chunks: chunks
                .iter()
                .map(|chunk| wire::SubmittedPatchChunk {
                    marker: chunk.marker.as_ref().map(evidence),
                    lines: chunk.lines.iter().map(line).collect(),
                    end_of_file: chunk.end_of_file.as_ref().map(evidence),
                })
                .collect(),
        },
        _ => return Err(Unsupported),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::Value;
    use serde_json::json;

    fn parsed_wire(source: &str) -> wire::SourceDescription {
        let input = activity::CompleteSourceDescriptionInput::from_source(
            "native-extension-closure".into(),
            activity::Language::TypeScript,
            source,
            activity::AnalysisLimits::default(),
        )
        .expect("bounded source");
        let mut analyzer = activity::Analyzer::new().expect("native parsers");
        let parsed = analyzer.describe_complete(input);
        assert_eq!(
            parsed.disposition(),
            activity::SourceDescriptionDisposition::CompleteSyntax
        );
        let converted = complete(&parsed)
            .unwrap_or_else(|_| panic!("complete source must convert to the protocol"));
        source_description_for_display(converted)
    }

    #[test]
    fn source_extensions_preserve_the_complete_wire_object() {
        // Hand-authored expectations shared with the restored/live TUI test.
        // Parsing this TypeScript -> shell -> Python source never executes it.
        let expected: Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../protocol/tests/fixtures/source_description_extensions.json"
        )))
        .expect("authored wire fixture");
        let source = expected["sources"][0]["text"]
            .as_str()
            .expect("outer TypeScript source");
        let actual = parsed_wire(source);
        assert_eq!(
            serde_json::to_value(&actual).expect("serialize converted source"),
            expected
        );
        assert_eq!(
            serde_json::from_value::<wire::SourceDescription>(expected)
                .expect("client wire round trip"),
            actual
        );
    }

    #[test]
    fn source_empty_owned_heredoc_preserves_its_parent_through_conversion() {
        let command = "python - <<'PY'\nPY\n";
        let literal = serde_json::to_string(command).expect("literal shell source");
        let source = format!("await tools.exec_command({{cmd:{literal}}});");
        let invocation = json!({
            "sourceId": 0, "startByte": 6, "endByte": source.len() - 1,
            "startLine": 1, "endLine": 1, "startColumn": 7
        });
        let actual = parsed_wire(&source);
        assert_eq!(
            serde_json::to_value(actual).expect("serialize empty owned source"),
            json!({
                "disposition": "completeSyntax",
                "sources": [
                    {"id": 0, "language": "typeScript", "parent": null, "text": source},
                    {"id": 1, "language": "shell", "parent": invocation, "text": command},
                    {
                        "id": 2, "language": "python", "text": "",
                        "parent": {
                            "sourceId": 1, "startByte": 16, "endByte": 16,
                            "startLine": 2, "endLine": 2, "startColumn": 1
                        }
                    }
                ],
                "references": [
                    {"kind": "invocationCall", "evidence": invocation, "literal": null},
                    {
                        "kind": "invocationCall", "literal": "python",
                        "evidence": {
                            "sourceId": 1, "startByte": 0, "endByte": 6,
                            "startLine": 1, "endLine": 1, "startColumn": 1
                        }
                    }
                ],
                "patches": []
            })
        );
    }
}
