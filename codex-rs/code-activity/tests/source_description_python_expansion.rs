//! Exposed alias regression and authored adversarial source-only cases.

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
        "python-open-alias".into(),
        Language::Python,
        source,
        AnalysisLimits::default(),
    )?;
    Ok(Analyzer::new()?.describe_complete(input))
}

#[test]
fn exposed_sd03_open_alias_retains_call_site_evidence() -> Result<(), ActivityError> {
    let source = "reader=open\nreader('paper/three.txt').read()";
    let result = describe(source)?;
    assert_eq!(
        result.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    let references: Vec<_> = result
        .references()
        .iter()
        .map(|reference| {
            (
                reference.kind,
                reference.literal.as_deref(),
                source.get(reference.evidence.start_byte..reference.evidence.end_byte),
            )
        })
        .collect();
    assert_eq!(
        references,
        vec![
            (
                SourceReferenceKind::OpenCall,
                Some("paper/three.txt"),
                Some("reader('paper/three.txt')")
            ),
            (
                SourceReferenceKind::ReadCall,
                Some("paper/three.txt"),
                Some("reader('paper/three.txt').read()")
            ),
        ]
    );
    assert!(
        result
            .references()
            .iter()
            .all(|reference| reference.basis == SourceReferenceBasis::SourceSyntax)
    );
    Ok(())
}

#[test]
fn immutable_callable_chain_supports_existing_read_replace_write_forms() -> Result<(), ActivityError>
{
    let source = "first = open\nsecond = (first)\nthird = second\np = 'a/./b'\ns = second(p, encoding='utf8').read()\ns = s.replace('old', 'new')\nthird(p, 'w', encoding='utf-8').write(s)";
    let result = describe(source)?;
    assert_eq!(
        result.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    assert_eq!(
        result
            .references()
            .iter()
            .map(|reference| reference.kind)
            .collect::<Vec<_>>(),
        vec![
            SourceReferenceKind::OpenCall,
            SourceReferenceKind::ReadCall,
            SourceReferenceKind::ReplaceCall,
            SourceReferenceKind::OpenCall,
            SourceReferenceKind::WriteCall,
        ]
    );
    assert!(
        result
            .references()
            .iter()
            .all(|reference| reference.literal.as_deref() == Some("a/./b"))
    );
    Ok(())
}

#[test]
fn open_name_aliases_work_in_existing_single_context_shape() -> Result<(), ActivityError> {
    let source = "reader = open\nwriter = reader\nwith reader('in', 'r') as source:\n    text = source.read()\nwith writer('out', 'w') as destination:\n    destination.write(text.replace('old', 'new'))";
    let result = describe(source)?;
    assert_eq!(
        result.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    assert_eq!(
        result
            .references()
            .iter()
            .filter(|reference| reference.kind == SourceReferenceKind::ReadCall
                || reference.kind == SourceReferenceKind::WriteCall)
            .map(|reference| (reference.kind, reference.literal.as_deref()))
            .collect::<Vec<_>>(),
        vec![
            (SourceReferenceKind::ReadCall, Some("in")),
            (SourceReferenceKind::WriteCall, Some("out")),
        ]
    );
    Ok(())
}

#[test]
fn alias_rebinding_mutation_or_escape_clears_every_reference() -> Result<(), ActivityError> {
    for suffix in [
        "reader = 'shadow'",
        "reader = open",
        "reader = open('b')",
        "open = 'shadow'",
        "reader.attribute = 'mutation'",
        "reader['key'] = 'mutation'",
        "del reader",
        "print(reader)",
        "helper(reader)",
        "container = [reader]",
        "container = {'reader': reader}",
        "data = 'old'\ndata = reader",
        "reader",
        "with reader('b') as reader:\n    reader.read()",
        "from other import reader",
        "def helper():\n    return reader",
    ] {
        let result = describe(&format!("reader = open\nreader('a').read()\n{suffix}"))?;
        assert!(
            matches!(
                result.disposition(),
                SourceDescriptionDisposition::Fallback(_)
            ),
            "{suffix}"
        );
        assert!(result.references().is_empty(), "{suffix}");
        assert!(result.sources().is_empty(), "{suffix}");
    }
    Ok(())
}

#[test]
fn data_handles_and_member_aliases_never_become_open_names() -> Result<(), ActivityError> {
    for source in [
        "reader = 'open'\nreader('a').read()",
        "reader = open('a')\nreader('b').read()",
        "handle = open('a')\nreader = handle\nreader('b').read()",
        "reader = open('a').read()\nreader('b').read()",
        "reader = open('a').read\nreader()",
        "from pathlib import Path\nreader = Path\nreader('a').read_text()",
        "reader = missing\nreader('a').read()",
        "open = 'shadow'\nreader = open\nreader('a').read()",
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
fn aliases_validate_complete_arguments_and_return_origins() -> Result<(), ActivityError> {
    for tail in [
        "reader(dynamic).read()",
        "reader('a', opener=callback).read()",
        "reader('a', encoding='custom').read()",
        "reader('a', **options).read()",
        "reader(*paths).read()",
        "reader('a', 'r', 1).read()",
        "reader('a').read(1)",
        "reader('a', 'w').read()",
        "reader('a').write('x')",
        "reader('a', 'w').write(reader)",
        "written = reader('a', 'w').write('x')",
    ] {
        let result = describe(&format!("reader = open\nopen('before').read()\n{tail}"))?;
        assert!(
            matches!(
                result.disposition(),
                SourceDescriptionDisposition::Fallback(_)
            ),
            "{tail}"
        );
        assert!(result.references().is_empty(), "{tail}");
    }
    Ok(())
}

#[test]
fn callable_alias_count_is_bounded() -> Result<(), ActivityError> {
    let mut source = "alias0 = open\n".to_owned();
    for index in 1..257 {
        source.push_str(&format!("alias{index} = alias{}\n", index - 1));
    }
    let result = describe(&source)?;
    assert_eq!(
        result.disposition(),
        SourceDescriptionDisposition::Fallback(SourceDescriptionFallback::SourceLimit)
    );
    assert!(result.references().is_empty());
    Ok(())
}

#[test]
fn unknown_runtime_alias_still_has_no_runtime_effect_rows() -> Result<(), ActivityError> {
    let source = "reader = open\nreader('a').read()";
    assert_eq!(
        describe(source)?.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    let input = CompleteScript::from_request(
        Request::new(
            "unknown-runtime".into(),
            Language::Python,
            source.into(),
            None,
        ),
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
