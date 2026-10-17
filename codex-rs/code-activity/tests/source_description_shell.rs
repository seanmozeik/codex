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
        "authored-shell-source".into(),
        Language::Shell,
        source,
        AnalysisLimits::default(),
    )?;
    Ok(Analyzer::new()?.describe_complete(input))
}

#[test]
fn ordinary_literal_commands_are_only_invocation_references() -> Result<(), ActivityError> {
    let description = describe("cat 'a b'; rm -- 'old.txt'\n# no runtime claim\nprintf '%s' done")?;
    assert_eq!(
        description.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    assert_eq!(
        description
            .references()
            .iter()
            .map(|reference| (
                reference.kind,
                reference.basis,
                reference.literal.as_deref(),
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                SourceReferenceKind::InvocationCall,
                SourceReferenceBasis::SourceSyntax,
                Some("cat")
            ),
            (
                SourceReferenceKind::InvocationCall,
                SourceReferenceBasis::SourceSyntax,
                Some("rm")
            ),
            (
                SourceReferenceKind::InvocationCall,
                SourceReferenceBasis::SourceSyntax,
                Some("printf")
            ),
        ],
    );
    assert_eq!(description.sources().len(), 1);
    Ok(())
}

#[test]
fn owned_python_source_recurses_through_supported_literal_wrappers() -> Result<(), ActivityError> {
    for source in [
        "python -c \"open('a').read()\"",
        "python3.12 -I -S -B -c \"open('a').read()\"",
        "env python -c \"open('a').read()\"",
        "command -- python -c \"open('a').read()\"",
        "uv run --offline python -c \"open('a').read()\"",
        "python <<'PY'\nopen('a').read()\nPY\n",
        "python - <<'PY'\nopen('a').read()\nPY\n",
    ] {
        let description = describe(source)?;
        assert_eq!(
            description.disposition(),
            SourceDescriptionDisposition::CompleteSyntax,
            "{source}"
        );
        assert_eq!(description.sources().len(), 2, "{source}");
        assert!(
            description
                .references()
                .iter()
                .any(|reference| reference.kind == SourceReferenceKind::ReadCall),
            "{source}"
        );
        for reference in description.references() {
            assert_eq!(reference.basis, SourceReferenceBasis::SourceSyntax);
        }
        let parent = description
            .sources()
            .get(1)
            .and_then(|child| child.parent.as_ref())
            .ok_or(ActivityError::SourceLimit)?;
        assert!(
            source.get(parent.start_byte..parent.end_byte).is_some(),
            "{source}"
        );
    }
    Ok(())
}

#[test]
fn closed_composition_environment_and_owned_input_promotions_are_complete()
-> Result<(), ActivityError> {
    for source in [
        "cat a && cat b",
        "cat a || cat b",
        "cat a | cat b",
        "cat a > out",
        "PYTHONPATH=x python -c \"open('a').read()\"",
        "env PYTHONPATH=x python -c \"open('a').read()\"",
        "python 0<<'PY'\nopen('a').read()\nPY\n",
        "python <<'PY' | cat\nopen('a').read()\nPY\n",
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
                .all(|reference| reference.basis == SourceReferenceBasis::SourceSyntax),
            "{source}"
        );
    }
    Ok(())
}

#[test]
fn unsupported_shell_or_child_suffix_discards_the_entire_description() -> Result<(), ActivityError>
{
    for source in [
        "cat a; mystery a",
        "cat a &",
        "cat a; alias python=custom",
        "cat a; python() { echo changed; }; python -c 'pass'",
        "cat a; eval 'cat b'",
        "cat a; . helper.sh",
        "cat $PATH",
        "cat *.txt",
        "echo \"$((1 + 2))\"",
        "echo \"$(cat a)\"",
        "python script.py",
        "python -m helper",
        "python --unknown -c \"open('a').read()\"",
        "python -c \"open('a').read()\" extra",
        "uv run script.py",
        "command python script.py",
        "env python script.py",
        "node script.js",
        "node --require helper -e 'unknown()'",
        "bash script.sh",
        "bash -c 'cat a; mystery b'",
        "python -c \"open('a').read(); mystery()\"",
        "python -c \"open('a').read(); x = (\"",
        "python <<PY\nopen('a').read()\nPY\n",
        "python <<-'PY'\nopen('a').read()\nPY\n",
        "python <<'PY' ignored\nopen('a').read()\nPY\n",
        "python <<'PY' && mystery\nopen('a').read()\nPY\n",
        "python <<'PY'\nopen('a').read(); mystery()\nPY\n",
        "python -c \"open('a').read()\" <<'PY'\nignored()\nPY\n",
        "python <<<\"open('a').read()\"",
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
fn recursive_shell_child_keeps_all_source_levels() -> Result<(), ActivityError> {
    let description = describe("bash -c 'cat a; rm -- b'")?;
    assert_eq!(
        description.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    assert_eq!(description.sources().len(), 2);
    assert_eq!(description.references().len(), 3);
    assert!(
        description
            .references()
            .iter()
            .all(|reference| reference.kind == SourceReferenceKind::InvocationCall)
    );
    Ok(())
}

#[test]
fn missing_inline_source_is_not_an_explicit_empty_script() -> Result<(), ActivityError> {
    for source in ["python -c", "node -e", "node -p", "python3 -I -c"] {
        let description = describe(source)?;
        assert_eq!(
            description.disposition(),
            SourceDescriptionDisposition::Fallback(SourceDescriptionFallback::UnsupportedSource),
            "{source}",
        );
        assert!(description.references().is_empty());
        assert!(description.sources().is_empty());
    }
    let empty = describe("python -c ''")?;
    assert_eq!(
        empty.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    Ok(())
}
