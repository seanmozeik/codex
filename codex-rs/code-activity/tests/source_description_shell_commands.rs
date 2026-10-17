//! Finite literal invocation vocabulary does not describe command effects.

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
        "authored-shell-commands".into(),
        Language::Shell,
        source,
        AnalysisLimits::default(),
    )?;
    Ok(Analyzer::new()?.describe_complete(input))
}

#[test]
fn git_cargo_and_sed_keep_only_invocation_spelling() -> Result<(), ActivityError> {
    let source = "git diff -- src; cargo test --lib; sed 's/old/new/g' settings.txt";
    let description = describe(source)?;
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
                Some("git")
            ),
            (
                SourceReferenceKind::InvocationCall,
                SourceReferenceBasis::SourceSyntax,
                Some("cargo")
            ),
            (
                SourceReferenceKind::InvocationCall,
                SourceReferenceBasis::SourceSyntax,
                Some("sed")
            ),
        ]
    );
    assert_eq!(description.sources().len(), 1);
    let spelling: Vec<_> = description
        .references()
        .iter()
        .map(|reference| source.get(reference.evidence.start_byte..reference.evidence.end_byte))
        .collect();
    assert_eq!(
        spelling,
        vec![
            Some("git diff -- src"),
            Some("cargo test --lib"),
            Some("sed 's/old/new/g' settings.txt")
        ]
    );
    Ok(())
}

#[test]
fn extended_names_are_a_finite_syntax_vocabulary() -> Result<(), ActivityError> {
    for name in [
        "git",
        "cargo",
        "rustc",
        "rustfmt",
        "clippy-driver",
        "just",
        "sed",
        "grep",
        "find",
        "wc",
        "sort",
        "uniq",
        "diff",
        "stat",
        "file",
        "du",
        "date",
        "uname",
        "which",
        "whoami",
        "realpath",
        "basename",
        "dirname",
        "readlink",
        "cut",
        "tr",
        "tee",
        "less",
        "more",
    ] {
        // Argument validity and executable identity are deliberately unasserted.
        let source = format!("{name} 'literal argument'");
        let description = describe(&source)?;
        assert_eq!(
            description.disposition(),
            SourceDescriptionDisposition::CompleteSyntax,
            "{source}"
        );
        assert_eq!(
            description
                .references()
                .iter()
                .map(|reference| (reference.kind, reference.literal.as_deref(),))
                .collect::<Vec<_>>(),
            vec![(SourceReferenceKind::InvocationCall, Some(name))]
        );
    }
    Ok(())
}

#[test]
fn closed_command_composition_promotions_keep_source_references() -> Result<(), ActivityError> {
    for source in [
        "git status && cargo test",
        "cargo test || git status",
        "git diff | wc -l",
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
fn unknown_or_conditional_suffix_discards_known_invocations() -> Result<(), ActivityError> {
    for source in [
        "git status; mystery",
        "cargo test; mystery",
        "sed 's/a/b/' file; mystery",
        "git status &",
        "git status; eval 'cargo test'",
        "git status; source helper.sh",
        "git status; . helper.sh",
        "git status; exec cargo test",
        "git status; export X=x",
        "git status; alias cargo=custom",
        "cargo() { echo shadow; }; cargo test",
        "git status; sed \"$RULE\" file",
        "cargo test $(cat arguments)",
        "wc *.rs",
        "git status; python script.py",
        "git status; node --require helper -e 'unknown()'",
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
fn program_basename_controls_carrier_grammar_without_identity_claims() -> Result<(), ActivityError>
{
    let ordinary = describe("/tmp/python/git status; /tmp/bash/cargo test")?;
    assert_eq!(
        ordinary.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    assert_eq!(
        ordinary
            .references()
            .iter()
            .map(|reference| reference.literal.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("/tmp/python/git"), Some("/tmp/bash/cargo")]
    );
    let nested = describe("/tmp/git/python -c \"open('a').read()\"")?;
    assert_eq!(
        nested.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    assert_eq!(nested.sources().len(), 2);
    assert!(
        nested
            .references()
            .iter()
            .any(|reference| reference.kind == SourceReferenceKind::ReadCall)
    );
    for source in [
        "/tmp/git/python script.py",
        "/tmp/cargo/bash script.sh",
        "/tmp/env python script.py",
        "/tmp/command python script.py",
        "/tmp/uv run python script.py",
        "python-helper -c 'cat a'",
        "cargo-helper test",
        "git status; /tmp/git/python -c \"open('a').read(); mystery()\"",
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
