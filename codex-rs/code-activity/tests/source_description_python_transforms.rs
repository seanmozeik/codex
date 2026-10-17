//! Closed text operators describe source origins, not runtime text or file edits.

#[path = "source_description_python_transforms/provenance.rs"]
mod provenance;

use codex_code_activity::ActivityError;
use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::CompleteScript;
use codex_code_activity::CompleteSourceDescriptionInput;
use codex_code_activity::EnvironmentAssumptions;
use codex_code_activity::Language;
use codex_code_activity::PresentationDisposition;
use codex_code_activity::Request;
use codex_code_activity::SourceDescription;
use codex_code_activity::SourceDescriptionDisposition;
use codex_code_activity::SourceDescriptionFallback;
use codex_code_activity::SourceReferenceBasis;
use codex_code_activity::SourceReferenceKind;
use pretty_assertions::assert_eq;

fn describe(source: &str) -> Result<SourceDescription, ActivityError> {
    let input = CompleteSourceDescriptionInput::from_source(
        "authored-python-transform".into(),
        Language::Python,
        source,
        AnalysisLimits::default(),
    )?;
    Ok(Analyzer::new()?.describe_complete(input))
}

#[test]
fn canonical_decimal_replace_counts_are_source_arguments() -> Result<(), ActivityError> {
    for count in ["0", "1", "12", "999999999999999999999999999999"] {
        let source = format!(
            "s = open('a').read()\ns = s.replace('old', 'new', {count})\nopen('a', 'w').write(s)"
        );
        let result = describe(&source)?;
        assert_eq!(
            result.disposition(),
            SourceDescriptionDisposition::CompleteSyntax,
            "{count}"
        );
        let replacement = result
            .references()
            .iter()
            .find(|reference| reference.kind == SourceReferenceKind::ReplaceCall)
            .ok_or(ActivityError::SourceLimit)?;
        assert_eq!(replacement.literal.as_deref(), Some("a"));
        assert_eq!(replacement.basis, SourceReferenceBasis::SourceSyntax);
        assert_eq!(
            source.get(replacement.evidence.start_byte..replacement.evidence.end_byte),
            Some(format!("s.replace('old', 'new', {count})").as_str())
        );
    }
    Ok(())
}

#[test]
fn p0_integer_replace_count_promotions_preserve_whole_source_rows() -> Result<(), ActivityError> {
    for count in ["+1", "00", "(1)"] {
        let before = "open('before').read()";
        let replacement = format!("open('a').read().replace('x', 'y', {count})");
        let source = format!("{before}\n{replacement}");
        let result = describe(&source)?;
        assert_eq!(
            result.disposition(),
            SourceDescriptionDisposition::CompleteSyntax,
            "{count}"
        );
        assert_eq!(
            result.source_texts(),
            std::slice::from_ref(&source),
            "{count}"
        );
        assert_eq!(result.sources().len(), 1, "{count}");
        assert!(result.patches().is_empty(), "{count}");
        let root = result.sources().first().ok_or(ActivityError::SourceLimit)?;
        assert_eq!(root.id.index(), 0);
        assert_eq!(root.language, Language::Python);
        assert!(root.parent.is_none());
        let rows: Vec<_> = result
            .references()
            .iter()
            .map(|reference| {
                assert_eq!(reference.basis, SourceReferenceBasis::SourceSyntax);
                let span = &reference.evidence;
                (
                    reference.kind,
                    reference.literal.as_deref(),
                    span.source_id.index(),
                    (
                        span.start_byte,
                        span.end_byte,
                        span.start_line,
                        span.end_line,
                        span.start_column,
                    ),
                    source.get(span.start_byte..span.end_byte),
                )
            })
            .collect();
        let second = before.len() + 1;
        assert_eq!(
            rows,
            vec![
                (
                    SourceReferenceKind::OpenCall,
                    Some("before"),
                    0,
                    (0, "open('before')".len(), 1, 1, 1),
                    Some("open('before')"),
                ),
                (
                    SourceReferenceKind::ReadCall,
                    Some("before"),
                    0,
                    (0, before.len(), 1, 1, 1),
                    Some(before),
                ),
                (
                    SourceReferenceKind::OpenCall,
                    Some("a"),
                    0,
                    (second, second + "open('a')".len(), 2, 2, 1),
                    Some("open('a')"),
                ),
                (
                    SourceReferenceKind::ReadCall,
                    Some("a"),
                    0,
                    (second, second + "open('a').read()".len(), 2, 2, 1),
                    Some("open('a').read()"),
                ),
                (
                    SourceReferenceKind::ReplaceCall,
                    Some("a"),
                    0,
                    (second, source.len(), 2, 2, 1),
                    Some(replacement.as_str()),
                ),
            ],
            "{count}"
        );
    }
    Ok(())
}

#[test]
fn dynamic_noncanonical_or_extra_replace_arguments_clear_the_whole_result()
-> Result<(), ActivityError> {
    for count in [
        "count",
        "unknown()",
        "-1",
        "01",
        "1_0",
        "0x1",
        "1.0",
        "True",
        "'1'",
        "count=1",
        "*counts",
        "**counts",
        "1, 2",
    ] {
        let source = format!("open('before').read()\nopen('a').read().replace('x', 'y', {count})");
        let result = describe(&source)?;
        assert!(
            matches!(
                result.disposition(),
                SourceDescriptionDisposition::Fallback(_)
            ),
            "{count}"
        );
        assert!(result.references().is_empty(), "{count}");
        assert!(result.sources().is_empty(), "{count}");
    }
    Ok(())
}
