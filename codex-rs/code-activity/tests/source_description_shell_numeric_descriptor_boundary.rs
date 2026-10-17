//! Inert developer regressions for descriptor ownership after the C extension.

use codex_code_activity::ActivityError;
use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::CompleteSourceDescriptionInput;
use codex_code_activity::Language;
use codex_code_activity::SourceDescription;
use codex_code_activity::SourceDescriptionDisposition;
use codex_code_activity::SourceDescriptionFallback;
use codex_code_activity::SourceReferenceBasis;
use codex_code_activity::SourceReferenceKind;
use std::time::Duration;

fn describe(source: &str) -> Result<SourceDescription, ActivityError> {
    let input = CompleteSourceDescriptionInput::from_source(
        "authored-numeric-descriptor-boundary".into(),
        Language::Shell,
        source,
        AnalysisLimits::new(32_768, Duration::from_secs(1)),
    )?;
    Ok(Analyzer::new()?.describe_complete(input))
}

#[test]
fn separated_or_quoted_digits_remain_arguments_before_a_redirect() -> Result<(), ActivityError> {
    for (source, command, redirect_start) in [
        ("cat 0 <file", "cat 0", 6),
        ("cat 00 <file", "cat 00", 7),
        ("cat '0'<file", "cat '0'", 7),
        ("cat '00'<file", "cat '00'", 8),
    ] {
        let description = describe(source)?;
        assert_eq!(
            description.disposition(),
            SourceDescriptionDisposition::CompleteSyntax,
            "{source}"
        );
        assert_eq!(description.source_texts(), [source.to_owned()]);
        assert_eq!(description.sources().len(), 1);
        assert!(description.patches().is_empty());
        let rows: Vec<_> = description
            .references()
            .iter()
            .map(|reference| {
                let evidence = &reference.evidence;
                assert_eq!(reference.basis, SourceReferenceBasis::SourceSyntax);
                assert_eq!(evidence.source_id.index(), 0);
                assert_eq!((evidence.start_line, evidence.end_line), (1, 1));
                assert_eq!(evidence.start_column, evidence.start_byte + 1);
                (
                    reference.kind,
                    reference.literal.as_deref(),
                    evidence.start_byte,
                    evidence.end_byte,
                )
            })
            .collect();
        assert_eq!(
            rows,
            vec![
                (
                    SourceReferenceKind::InvocationCall,
                    Some("cat"),
                    0,
                    command.len()
                ),
                (
                    SourceReferenceKind::ShellRedirection,
                    Some("file"),
                    redirect_start,
                    source.len(),
                ),
            ],
            "{source}"
        );
    }
    Ok(())
}

#[test]
fn unsupported_adjacent_descriptors_never_become_numeric_arguments() -> Result<(), ActivityError> {
    for tail in ["cat 00<file", "cat 12<file"] {
        let source = format!("git status; {tail}");
        let description = describe(&source)?;
        assert_eq!(
            description.disposition(),
            SourceDescriptionDisposition::Fallback(SourceDescriptionFallback::UnsupportedSource),
            "{source}"
        );
        assert!(description.sources().is_empty(), "{source}");
        assert!(description.source_texts().is_empty(), "{source}");
        assert!(description.references().is_empty(), "{source}");
        assert!(description.patches().is_empty(), "{source}");
    }
    Ok(())
}
