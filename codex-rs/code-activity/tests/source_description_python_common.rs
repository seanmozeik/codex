//! Whole-source syntax references and rejection boundary regressions.

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
use pretty_assertions::assert_eq;

fn describe(source: &str) -> Result<SourceDescription, ActivityError> {
    let input = CompleteSourceDescriptionInput::from_source(
        "authored-common-python".into(),
        Language::Python,
        source,
        AnalysisLimits::default(),
    )?;
    Ok(Analyzer::new()?.describe_complete(input))
}

#[test]
fn self_replacement_retains_read_origin_and_earlier_alias() -> Result<(), ActivityError> {
    let source = "s = open('a').read()\nold = s\ns = s.replace('x', 'y')\ns = open('b').read()\nprint(old)\nopen('out', 'w').write(s)";
    let result = describe(source)?;
    assert_eq!(
        result.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    let references: Vec<_> = result
        .references()
        .iter()
        .map(|reference| (reference.kind, reference.literal.as_deref()))
        .collect();
    assert_eq!(
        references,
        vec![
            (SourceReferenceKind::OpenCall, Some("a")),
            (SourceReferenceKind::ReadCall, Some("a")),
            (SourceReferenceKind::ReplaceCall, Some("a")),
            (SourceReferenceKind::OpenCall, Some("b")),
            (SourceReferenceKind::ReadCall, Some("b")),
            (SourceReferenceKind::OutputConsumer, None),
            (SourceReferenceKind::OpenCall, Some("out")),
            (SourceReferenceKind::WriteCall, Some("out")),
        ]
    );
    let alias = describe(
        "s = open('a').read()\nt = s\ns = 'new'\nopen('out','w').write(t.replace('x','y'))",
    )?;
    assert!(alias.references().iter().any(|reference| reference.kind
        == SourceReferenceKind::ReplaceCall
        && reference.literal.as_deref() == Some("a")));
    Ok(())
}

#[test]
fn supported_literal_encoding_is_checked_for_each_supported_api() -> Result<(), ActivityError> {
    for source in [
        "open('a', encoding='utf-8').read()",
        "open('a', 'w', encoding='utf8').write('x')",
        "from pathlib import Path\np = Path('a')\ns = p.read_text(encoding='utf-8')\ns = s.replace('a','b')\np.write_text(s, encoding='utf8')",
    ] {
        let result = describe(source)?;
        assert_eq!(
            result.disposition(),
            SourceDescriptionDisposition::CompleteSyntax,
            "{source}"
        );
        assert!(
            result
                .references()
                .iter()
                .all(|reference| reference.basis == SourceReferenceBasis::SourceSyntax)
        );
    }
    Ok(())
}

#[test]
fn single_context_read_data_can_feed_later_context_write() -> Result<(), ActivityError> {
    let source = "with open('a', 'r', encoding='utf-8') as reader:\n    s = reader.read()\ns = s.replace('old', 'new')\nwith open('b', 'w', encoding='utf8') as writer:\n    writer.write(s)\nprint('done', s)";
    let result = describe(source)?;
    assert_eq!(
        result.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    let kinds: Vec<_> = result
        .references()
        .iter()
        .map(|reference| reference.kind)
        .collect();
    assert_eq!(
        kinds,
        vec![
            SourceReferenceKind::OpenCall,
            SourceReferenceKind::ReadCall,
            SourceReferenceKind::ReplaceCall,
            SourceReferenceKind::OpenCall,
            SourceReferenceKind::WriteCall,
            SourceReferenceKind::OutputConsumer,
        ]
    );
    Ok(())
}

#[test]
fn context_handles_cannot_escape_or_be_reused_after_context() -> Result<(), ActivityError> {
    for source in [
        "with open('a') as f:\n    alias = f\nalias.read()",
        "with open('a') as f:\n    s = f.read()\nf.read()",
        "with open('a') as f:\n    s = f.read()\nf = 'replacement'",
        "with open('a') as f:\n    f = 'replacement'",
        "with open('a') as f, open('b') as g:\n    f.read()",
        "with open('a') as f:\n    with open('b') as g:\n        g.read()",
        "async with open('a') as f:\n    f.read()",
        "f = open('a')\nwith f as g:\n    g.read()",
        "with open('a') as print:\n    print.read()",
    ] {
        let result = describe(source)?;
        assert!(
            matches!(
                result.disposition(),
                SourceDescriptionDisposition::Fallback(_)
            ),
            "{source}"
        );
        assert!(result.references().is_empty(), "{source}");
    }
    Ok(())
}

#[test]
fn unknown_options_output_consumers_and_api_rebinding_clear_all_rows() -> Result<(), ActivityError>
{
    for suffix in [
        "open('b', encoding='custom_codec').read()",
        "encoding = 'utf8'\nopen('b', encoding=encoding).read()",
        "open('b', encoding='utf8', errors='ignore').read()",
        "open('b', encoding='utf8', encoding='utf8').read()",
        "open('b').read(encoding='utf8')",
        "open('b', 'w').write('x', encoding='utf8')",
        "print(s, file=unknown)",
        "print(s, sep='')",
        "print(f'{s}')",
        "print(unknown())",
        "printed = print(s)",
        "print = 'shadow'",
        "s = open('b')",
        "open = 'shadow'",
    ] {
        let result = describe(&format!("s = open('a').read()\n{suffix}"))?;
        assert!(
            matches!(
                result.disposition(),
                SourceDescriptionDisposition::Fallback(_)
            ),
            "{suffix}"
        );
        assert!(result.references().is_empty(), "{suffix}");
    }
    Ok(())
}

#[test]
fn reassignments_do_not_refund_retained_byte_budget() -> Result<(), ActivityError> {
    let source = format!("s = '{}'\n{}", "x".repeat(16_000), "s = s\n".repeat(70));
    let result = describe(&source)?;
    assert_eq!(
        result.disposition(),
        SourceDescriptionDisposition::Fallback(SourceDescriptionFallback::SourceLimit)
    );
    assert!(result.references().is_empty());
    Ok(())
}
