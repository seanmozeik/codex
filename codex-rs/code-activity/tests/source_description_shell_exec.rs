//! Literal exec wrappers describe source spelling, never process replacement.

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
use pretty_assertions::assert_ne;

fn describe(source: &str) -> Result<SourceDescription, ActivityError> {
    let input = CompleteSourceDescriptionInput::from_source(
        "authored-exec-source".into(),
        Language::Shell,
        source,
        AnalysisLimits::default(),
    )?;
    Ok(Analyzer::new()?.describe_complete(input))
}

#[test]
fn literal_exec_accepts_supported_direct_interpreter_source() -> Result<(), ActivityError> {
    for (source, language) in [
        (
            "exec /authored/bin/python3 -S -B -c \"open('a').read()\"",
            Language::Python,
        ),
        (
            "exec node -e \"const fs = require('fs'); fs.readFileSync('a', 'utf8');\"",
            Language::TypeScript,
        ),
        ("exec bash -c 'cat a'", Language::Shell),
        (
            "exec python <<'PY'\nopen('a').read()\nPY\n",
            Language::Python,
        ),
        (
            "exec python - <<'PY'\nopen('a').read()\nPY\n",
            Language::Python,
        ),
    ] {
        let description = describe(source)?;
        assert_eq!(
            description.disposition(),
            SourceDescriptionDisposition::CompleteSyntax,
            "{source}"
        );
        assert_eq!(description.sources().len(), 2, "{source}");
        let child = description
            .sources()
            .get(1)
            .ok_or(ActivityError::SourceLimit)?;
        assert_eq!(child.language, language, "{source}");
        let invocation = description
            .references()
            .first()
            .ok_or(ActivityError::SourceLimit)?;
        assert_eq!(invocation.kind, SourceReferenceKind::InvocationCall);
        assert_eq!(invocation.literal.as_deref(), Some("exec"));
        assert!(
            description
                .references()
                .iter()
                .all(|reference| reference.basis == SourceReferenceBasis::SourceSyntax)
        );
    }
    Ok(())
}

#[test]
fn exec_inline_child_parent_span_accounts_for_removed_word() -> Result<(), ActivityError> {
    for (source, payload) in [
        (
            "exec /authored/bin/python3 -S -B -c \"open('a').read()\"",
            "\"open('a').read()\"",
        ),
        (
            "exec python -c\"open('compact').read()\"",
            "-c\"open('compact').read()\"",
        ),
    ] {
        let description = describe(source)?;
        let parent = description
            .sources()
            .get(1)
            .and_then(|child| child.parent.as_ref())
            .ok_or(ActivityError::SourceLimit)?;
        assert_eq!(
            source.get(parent.start_byte..parent.end_byte),
            Some(payload)
        );
        let read = description
            .references()
            .iter()
            .find(|reference| reference.kind == SourceReferenceKind::ReadCall)
            .ok_or(ActivityError::SourceLimit)?;
        assert_ne!(read.evidence.source_id, parent.source_id);
    }
    Ok(())
}

#[test]
fn literal_exec_context_and_composition_promotions_keep_owned_children() -> Result<(), ActivityError>
{
    for source in [
        "VAR=value exec python -c \"open('a').read()\"",
        "cat before; exec python -c \"open('a').read()\" && cat after",
        "cat before; exec python -c \"open('a').read()\" > output",
    ] {
        let description = describe(source)?;
        assert_eq!(
            description.disposition(),
            SourceDescriptionDisposition::CompleteSyntax,
            "{source}"
        );
        assert!(!description.sources().is_empty(), "{source}");
        assert!(!description.references().is_empty(), "{source}");
        assert_eq!(
            description.source_texts().first().map(String::as_str),
            Some(source)
        );
        assert!(
            description
                .references()
                .iter()
                .any(|reference| reference.kind == SourceReferenceKind::ReadCall),
            "{source}"
        );
        assert!(
            description
                .references()
                .iter()
                .all(|reference| reference.basis == SourceReferenceBasis::SourceSyntax),
            "{source}"
        );
    }
    Ok(())
}

#[test]
fn exec_options_wrappers_and_opaque_children_reject_the_whole_source() -> Result<(), ActivityError>
{
    for source in [
        "exec",
        "exec -l python -c \"open('a').read()\"",
        "exec -- python -c \"open('a').read()\"",
        "exec -a renamed python -c \"open('a').read()\"",
        "exec env python -c \"open('a').read()\"",
        "exec uv run python -c \"open('a').read()\"",
        "exec command python -c \"open('a').read()\"",
        "exec exec python -c \"open('a').read()\"",
        "/authored/exec python -c \"open('a').read()\"",
        "exec cat a",
        "exec mystery -c \"open('a').read()\"",
        "exec PYTHONPATH=custom python -c \"open('a').read()\"",
        "exec python script.py",
        "exec python -m module",
        "exec python -c",
        "exec python",
        "exec python -c \"open('a').read()\" extra",
        "exec python --unknown -c \"open('a').read()\"",
        "exec $PYTHON -c \"open('a').read()\"",
        "exec python -c \"$CODE\"",
        "exec python -c \"$(cat code)\"",
        "exec python <<PY\nopen('a').read()\nPY\n",
        "exec python <<'PY'\nopen('a').read(); unknown()\nPY\n",
    ] {
        let description = describe(source)?;
        assert_eq!(
            description.disposition(),
            SourceDescriptionDisposition::Fallback(SourceDescriptionFallback::UnsupportedSource),
            "{source}"
        );
        assert!(description.references().is_empty(), "{source}");
        assert!(description.sources().is_empty(), "{source}");
    }
    Ok(())
}

#[test]
fn unsupported_exec_child_or_suffix_clears_preceding_known_statement() -> Result<(), ActivityError>
{
    for source in [
        "cat before; exec python -c \"open('a').read(); unknown()\"",
        "cat before; exec python -c \"open('a').read()\"; mystery after",
    ] {
        let description = describe(source)?;
        assert_eq!(
            description.disposition(),
            SourceDescriptionDisposition::Fallback(SourceDescriptionFallback::UnsupportedSource),
            "{source}"
        );
        assert!(description.references().is_empty(), "{source}");
        assert!(description.sources().is_empty(), "{source}");
    }
    Ok(())
}

#[test]
fn exact_capital_python_alias_preserves_original_invocation_and_span() -> Result<(), ActivityError>
{
    for (source, invocation) in [
        ("Python -c \"open('a').read()\"", "Python"),
        (
            "/authored/Python -S -B -c \"open('a').read()\"",
            "/authored/Python",
        ),
        ("exec Python -c \"open('a').read()\"", "exec"),
        (
            "exec /authored/Python -S -B -c \"open('a').read()\"",
            "exec",
        ),
    ] {
        let description = describe(source)?;
        assert_eq!(
            description.disposition(),
            SourceDescriptionDisposition::CompleteSyntax,
            "{source}"
        );
        let reference = description
            .references()
            .first()
            .ok_or(ActivityError::SourceLimit)?;
        assert_eq!(reference.literal.as_deref(), Some(invocation));
        assert_eq!(reference.basis, SourceReferenceBasis::SourceSyntax);
        let child = description
            .sources()
            .get(1)
            .ok_or(ActivityError::SourceLimit)?;
        assert_eq!(child.language, Language::Python);
        let parent = child.parent.as_ref().ok_or(ActivityError::SourceLimit)?;
        assert_eq!(
            source.get(parent.start_byte..parent.end_byte),
            Some("\"open('a').read()\"")
        );
    }
    Ok(())
}

#[test]
fn capital_python_alias_does_not_broaden_cases_wrappers_or_child_grammar()
-> Result<(), ActivityError> {
    for source in [
        "PYTHON -c \"open('a').read()\"",
        "exec /authored/PYTHON -c \"open('a').read()\"",
        "Pythons -c \"open('a').read()\"",
        "exec /authored/Pythons -c \"open('a').read()\"",
        "Python",
        "exec Python -c",
        "Python --unknown -c \"open('a').read()\"",
        "exec /authored/Python --unknown -c \"open('a').read()\"",
        "env Python -c \"open('a').read()\"",
        "exec env Python -c \"open('a').read()\"",
        "cat before; Python -c \"open('a').read(); unknown()\"",
        "cat before; exec /authored/Python -c \"open('a').read(); unknown()\"",
    ] {
        let description = describe(source)?;
        assert_eq!(
            description.disposition(),
            SourceDescriptionDisposition::Fallback(SourceDescriptionFallback::UnsupportedSource),
            "{source}"
        );
        assert!(description.references().is_empty(), "{source}");
        assert!(description.sources().is_empty(), "{source}");
    }
    Ok(())
}
