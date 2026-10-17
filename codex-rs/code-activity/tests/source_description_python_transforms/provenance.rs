use super::ActivityError;
use super::AnalysisLimits;
use super::Analyzer;
use super::CompleteScript;
use super::EnvironmentAssumptions;
use super::Language;
use super::PresentationDisposition;
use super::Request;
use super::SourceDescriptionDisposition;
use super::SourceDescriptionFallback;
use super::SourceReferenceKind;
use super::assert_eq;
use super::describe;

#[test]
fn text_plus_literals_preserves_a_single_read_source_reference() -> Result<(), ActivityError> {
    for expression in [
        "s + '\\n'",
        "'prefix' + s",
        "s + s",
        "s.replace('x','y') + 'suffix'",
    ] {
        let source = format!(
            "s = open('a/./b').read()\nt = {expression}\nt = t.replace('a','b',1)\nopen('out','w').write(t)"
        );
        let result = describe(&source)?;
        assert_eq!(
            result.disposition(),
            SourceDescriptionDisposition::CompleteSyntax,
            "{expression}"
        );
        assert!(
            result
                .references()
                .iter()
                .filter(|reference| reference.kind == SourceReferenceKind::ReplaceCall)
                .all(|reference| reference.literal.as_deref() == Some("a/./b"))
        );
    }
    Ok(())
}

#[test]
fn mixed_read_sources_remain_unknown_through_later_transforms() -> Result<(), ActivityError> {
    for expression in [
        "mixed",
        "mixed + a",
        "a + mixed",
        "mixed + 'suffix'",
        "mixed.replace('x','y') + a",
    ] {
        let source = format!(
            "a = open('first').read()\nb = open('second').read()\nmixed = a + b\nresult = {expression}\nresult = result.replace('x','y')\nopen('out','w').write(result)"
        );
        let result = describe(&source)?;
        assert_eq!(
            result.disposition(),
            SourceDescriptionDisposition::CompleteSyntax,
            "{expression}"
        );
        let replacements: Vec<_> = result
            .references()
            .iter()
            .filter(|reference| reference.kind == SourceReferenceKind::ReplaceCall)
            .collect();
        assert!(!replacements.is_empty());
        assert!(
            replacements
                .iter()
                .all(|reference| reference.literal.is_none()),
            "{expression}"
        );
        assert!(
            result
                .references()
                .iter()
                .any(|reference| reference.kind == SourceReferenceKind::WriteCall
                    && reference.literal.as_deref() == Some("out"))
        );
    }
    Ok(())
}

#[test]
fn literal_composition_is_data_but_never_a_new_path_literal() -> Result<(), ActivityError> {
    assert_eq!(
        describe("text = 'a' + 'b'\nopen('out','w').write(text)")?.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    for source in [
        "open('a' + 'b').read()",
        "path = 'a' + 'b'\nopen(path).read()",
        "from pathlib import Path\nPath('a' + 'b').read_text()",
        "from pathlib import Path\npath = 'a' + 'b'\nPath(path).read_text()",
    ] {
        let result = describe(source)?;
        assert_eq!(
            result.disposition(),
            SourceDescriptionDisposition::Fallback(SourceDescriptionFallback::UnsupportedSource),
            "{source}"
        );
        assert!(result.references().is_empty());
    }
    Ok(())
}

#[test]
fn operators_cannot_coerce_nontext_origins_or_skip_unknown_calls() -> Result<(), ActivityError> {
    for expression in [
        "s + 1",
        "1 + s",
        "s + True",
        "s + None",
        "s + open",
        "s + open('other')",
        "s + unknown()",
        "s + print('x')",
        "s + open('out','w').write('x')",
        "s - 'x'",
        "s * 2",
        "s / 'x'",
        "s % 'x'",
        "s | 'x'",
        "s and 'x'",
    ] {
        let result = describe(&format!(
            "s = open('a').read()\nt = {expression}\nopen('out','w').write(t)"
        ))?;
        assert!(
            matches!(
                result.disposition(),
                SourceDescriptionDisposition::Fallback(_)
            ),
            "{expression}"
        );
        assert!(result.references().is_empty(), "{expression}");
    }
    let path = describe("from pathlib import Path\np = Path('a')\ntext = p + 'suffix'")?;
    assert!(path.references().is_empty());
    Ok(())
}

#[test]
fn closed_data_alias_keeps_prior_provenance_after_reassignment() -> Result<(), ActivityError> {
    let result = describe(
        "a = open('first').read()\nb = open('second').read()\nold = a\na = a + b\nopen('out','w').write(old.replace('x','y',1))",
    )?;
    assert_eq!(
        result.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    let replacement = result
        .references()
        .iter()
        .find(|reference| reference.kind == SourceReferenceKind::ReplaceCall)
        .ok_or(ActivityError::SourceLimit)?;
    assert_eq!(replacement.literal.as_deref(), Some("first"));
    Ok(())
}

#[test]
fn supported_text_syntax_does_not_grant_runtime_effect_eligibility() -> Result<(), ActivityError> {
    let source = "s = open('a').read()\ns = s.replace('a','b',1) + '\\n'\nopen('a','w').write(s)";
    assert_eq!(
        describe(source)?.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    let input = CompleteScript::from_request(
        Request::new("unknown".into(), Language::Python, source.into(), None),
        EnvironmentAssumptions::PersistentOrUnknown,
        AnalysisLimits::default(),
    )?;
    let result = Analyzer::new()?.analyze_complete(input);
    assert!(matches!(
        result.disposition(),
        PresentationDisposition::Fallback(_)
    ));
    assert!(result.ui_rows().is_empty());
    Ok(())
}

#[test]
fn composed_origins_are_charged_to_the_cumulative_byte_bound() -> Result<(), ActivityError> {
    let source = format!(
        "s = open('{}').read()\n{}",
        "x".repeat(16_000),
        "s = s + 'suffix'\n".repeat(35)
    );
    let result = describe(&source)?;
    assert_eq!(
        result.disposition(),
        SourceDescriptionDisposition::Fallback(SourceDescriptionFallback::SourceLimit)
    );
    assert!(result.references().is_empty());
    Ok(())
}
