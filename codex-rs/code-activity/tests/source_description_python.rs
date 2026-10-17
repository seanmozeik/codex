//! Whole-source syntax references and rejection boundary regressions.

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
use std::time::Duration;

fn describe(source: &str) -> Result<SourceDescription, ActivityError> {
    let input = CompleteSourceDescriptionInput::from_source(
        "authored-source".into(),
        Language::Python,
        source,
        AnalysisLimits::default(),
    )?;
    Ok(Analyzer::new()?.describe_complete(input))
}

#[test]
fn pathlib_source_retains_import_and_literal_spelling() -> Result<(), ActivityError> {
    let source = "from pathlib import Path as P\np = P('a/./b')\np.write_text(p.read_text().replace('old', 'new'))";
    let description = describe(source)?;
    assert_eq!(
        description.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    assert_eq!(
        description
            .references()
            .iter()
            .map(|reference| reference.kind)
            .collect::<Vec<_>>(),
        vec![
            SourceReferenceKind::ImportFrom,
            SourceReferenceKind::PathConstructorCall,
            SourceReferenceKind::ReadCall,
            SourceReferenceKind::ReplaceCall,
            SourceReferenceKind::WriteCall,
        ]
    );
    for reference in description.references() {
        assert_eq!(reference.basis, SourceReferenceBasis::SourceSyntax);
        assert!(
            source
                .get(reference.evidence.start_byte..reference.evidence.end_byte)
                .is_some()
        );
        assert_eq!(
            reference.literal.as_deref(),
            Some(if reference.kind == SourceReferenceKind::ImportFrom {
                "pathlib.Path"
            } else {
                "a/./b"
            })
        );
    }
    Ok(())
}

#[test]
fn literal_owned_bindings_and_cross_path_calls_are_source_references() -> Result<(), ActivityError>
{
    let description = describe(
        "path = 'input'\nreader = open(path)\ntext = reader.read()\nchanged = text.replace('a', 'b')\nwriter = open('output', 'w')\nwriter.write(changed)",
    )?;
    assert_eq!(
        description.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    let calls: Vec<_> = description
        .references()
        .iter()
        .map(|reference| (reference.kind, reference.literal.as_deref()))
        .collect();
    assert_eq!(
        calls,
        vec![
            (SourceReferenceKind::OpenCall, Some("input")),
            (SourceReferenceKind::ReadCall, Some("input")),
            (SourceReferenceKind::ReplaceCall, Some("input")),
            (SourceReferenceKind::OpenCall, Some("output")),
            (SourceReferenceKind::WriteCall, Some("output")),
        ]
    );
    Ok(())
}

#[test]
fn literal_text_replacement_is_not_folded_into_a_path() -> Result<(), ActivityError> {
    let source =
        "text = 'old'\nchanged = text.replace('old', 'new')\nopen('out', 'w').write(changed)";
    assert_eq!(
        describe(source)?.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    let unsupported = "path = 'old'.replace('old', 'new')\nopen(path).read()";
    assert!(describe(unsupported)?.references().is_empty());
    Ok(())
}

#[test]
fn unsupported_prefix_or_suffix_discards_every_reference() -> Result<(), ActivityError> {
    for unsupported in [
        "unknown()",
        "import custom",
        "open = 'shadow'",
        "item.x = 'mutation'",
        "assert 'condition'",
        "for item in ['x']:\n    open(item).read()",
        "def callback():\n    open('hidden').read()",
    ] {
        for source in [
            format!("{unsupported}\nopen('visible').read()"),
            format!("open('visible').read()\n{unsupported}"),
        ] {
            let description = describe(&source)?;
            assert!(
                matches!(
                    description.disposition(),
                    SourceDescriptionDisposition::Fallback(_)
                ),
                "{source}"
            );
            assert!(description.references().is_empty(), "{source}");
            assert!(description.sources().is_empty(), "{source}");
        }
    }
    Ok(())
}

#[test]
fn unsupported_arguments_and_rebindings_do_not_escape_as_partial_rows() -> Result<(), ActivityError>
{
    for source in [
        "open('a', opener=callback).read()",
        "open('a', 'r', callback()).read()",
        "open(dynamic_path).read()",
        "open('a' + 'b').read()",
        "open(f'{name}').read()",
        "open(b'a').read()",
        "open('a', 'rb').read()",
        "open('a').read(2)",
        "open('a').read().replace('a', unknown())",
        "open('a').read().replace('a', 'b', dynamic_count)",
        "reader = open('a')\nreader = open('b')\nreader.read()",
        "from pathlib import Path\nPath = 'shadow'\nPath('a').read_text()",
        "from pathlib import Path as open\nopen('a').read_text()",
        "from pathlib import Path\np = Path('a')\nr = p.read_text\nr()",
        "count = open('a', 'w').write('x')",
        "from pathlib import Path\nimport other\nPath('a').read_text()",
    ] {
        let description = describe(source)?;
        assert!(
            matches!(
                description.disposition(),
                SourceDescriptionDisposition::Fallback(_)
            ),
            "{source}"
        );
        assert!(description.references().is_empty(), "{source}");
    }
    Ok(())
}

#[test]
fn syntax_description_cannot_promote_unknown_runtime_effects() -> Result<(), ActivityError> {
    let source = "from pathlib import Path\nPath('a').write_text('x')";
    assert_eq!(
        describe(source)?.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    let input = CompleteScript::from_request(
        Request::new("same-source".into(), Language::Python, source.into(), None),
        EnvironmentAssumptions::PersistentOrUnknown,
        AnalysisLimits::default(),
    )?;
    let activity = Analyzer::new()?.analyze_complete(input);
    assert!(matches!(
        activity.disposition(),
        PresentationDisposition::Fallback(_)
    ));
    assert!(activity.ui_rows().is_empty());
    Ok(())
}

#[test]
fn byte_node_depth_and_reference_limits_discard_the_whole_description() -> Result<(), ActivityError>
{
    let too_large = "x".repeat(32 * 1024 + 1);
    assert!(matches!(
        CompleteSourceDescriptionInput::from_source(
            "large".into(),
            Language::Python,
            &too_large,
            AnalysisLimits::default()
        ),
        Err(ActivityError::SourceLimit)
    ));
    for source in [
        "open('a').read()\n".repeat(130),
        "'x'\n".repeat(2200),
        format!("{}'x'{}", "(".repeat(70), ")".repeat(70)),
    ] {
        let result = describe(&source)?;
        assert_eq!(
            result.disposition(),
            SourceDescriptionDisposition::Fallback(SourceDescriptionFallback::SourceLimit)
        );
        assert!(result.references().is_empty());
    }
    Ok(())
}

#[test]
fn expired_request_does_not_affect_the_next_description() -> Result<(), ActivityError> {
    let mut analyzer = Analyzer::new()?;
    let expired = CompleteSourceDescriptionInput::from_source(
        "expired".into(),
        Language::Python,
        "open('a').read()",
        AnalysisLimits::new(1024, Duration::ZERO),
    )?;
    let result = analyzer.describe_complete(expired);
    assert_eq!(
        result.disposition(),
        SourceDescriptionDisposition::Fallback(SourceDescriptionFallback::TimeBudget)
    );
    assert!(result.references().is_empty());
    let next = CompleteSourceDescriptionInput::from_source(
        "next".into(),
        Language::Python,
        "open('fresh').read()",
        AnalysisLimits::default(),
    )?;
    assert_eq!(
        analyzer.describe_complete(next).disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    Ok(())
}
