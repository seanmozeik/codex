//! Inert regressions for trailing atom file redirects in composition.

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
        "authored-shell-file-redirect-ownership".into(),
        Language::Shell,
        source,
        AnalysisLimits::default(),
    )?;
    Ok(Analyzer::new()?.describe_complete(input))
}

fn rows(description: &SourceDescription) -> Vec<(SourceReferenceKind, Option<&str>, usize, &str)> {
    description
        .references()
        .iter()
        .map(|reference| {
            let evidence = &reference.evidence;
            let source = &description.source_texts()[evidence.source_id.index()];
            assert_eq!(reference.basis, SourceReferenceBasis::SourceSyntax);
            (
                reference.kind,
                reference.literal.as_deref(),
                evidence.source_id.index(),
                &source[evidence.start_byte..evidence.end_byte],
            )
        })
        .collect()
}

#[test]
fn pipeline_tail_retains_ordered_invocations_and_complete_utf8_redirect_evidence()
-> Result<(), ActivityError> {
    let source = "head -n 0040 notes.txt | tail -n -5 > 'é out' 2>&1";
    let description = describe(source)?;
    assert_eq!(
        description.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    assert_eq!(description.source_texts(), &[source.to_owned()]);
    assert_eq!(description.sources().len(), 1);
    assert!(description.patches().is_empty());
    assert_eq!(
        rows(&description),
        vec![
            (
                SourceReferenceKind::InvocationCall,
                Some("head"),
                0,
                "head -n 0040 notes.txt"
            ),
            (SourceReferenceKind::ShellConnector, Some("|"), 0, "|"),
            (
                SourceReferenceKind::InvocationCall,
                Some("tail"),
                0,
                "tail -n -5"
            ),
            (
                SourceReferenceKind::ShellRedirection,
                Some("é out"),
                0,
                "> 'é out'"
            ),
            (SourceReferenceKind::ShellRedirection, None, 0, "2>&1"),
        ]
    );
    Ok(())
}

#[test]
fn attachment_follows_the_final_atom_without_discarding_conditions_or_prior_groups()
-> Result<(), ActivityError> {
    let source = "pwd && cat | tail -n 2 >>out";
    let description = describe(source)?;
    assert_eq!(
        description.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    assert_eq!(description.source_texts(), &[source.to_owned()]);
    assert_eq!(description.sources().len(), 1);
    assert_eq!(
        rows(&description),
        vec![
            (SourceReferenceKind::InvocationCall, Some("pwd"), 0, "pwd"),
            (SourceReferenceKind::ShellConnector, Some("&&"), 0, "&&"),
            (SourceReferenceKind::InvocationCall, Some("cat"), 0, "cat"),
            (SourceReferenceKind::ShellConnector, Some("|"), 0, "|"),
            (
                SourceReferenceKind::InvocationCall,
                Some("tail"),
                0,
                "tail -n 2"
            ),
            (
                SourceReferenceKind::ShellRedirection,
                Some("out"),
                0,
                ">>out"
            ),
        ]
    );

    let grouped = "(pwd) | cat >out";
    let description = describe(grouped)?;
    assert_eq!(
        description.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    assert_eq!(description.source_texts(), &[grouped.to_owned()]);
    assert_eq!(description.sources().len(), 1);
    assert_eq!(
        rows(&description),
        vec![
            (SourceReferenceKind::ShellConnector, Some("("), 0, "("),
            (SourceReferenceKind::InvocationCall, Some("pwd"), 0, "pwd"),
            (SourceReferenceKind::ShellConnector, Some(")"), 0, ")"),
            (SourceReferenceKind::ShellConnector, Some("|"), 0, "|"),
            (SourceReferenceKind::InvocationCall, Some("cat"), 0, "cat"),
            (
                SourceReferenceKind::ShellRedirection,
                Some("out"),
                0,
                ">out"
            ),
        ]
    );
    Ok(())
}

#[test]
fn grouped_owners_unknown_siblings_and_unsupported_redirects_erase_the_entire_source()
-> Result<(), ActivityError> {
    for tail in [
        "(pwd | cat) >out",
        "{ pwd | cat; } >out",
        "pwd | (cat) >out",
        "pwd || { cat; } >out",
        "mystery | cat >out",
        "pwd | mystery | cat >out",
        "pwd | mystery >out",
        "pwd |& cat >out",
        "pwd & cat >out",
        "! pwd | cat >out",
        "pwd | cat &>out",
        "pwd | cat >|out",
        "pwd | cat >&-",
        "pwd | cat 3>out",
        "pwd | cat 12>out",
        "pwd | cat 00<in",
        "pwd | cat >\"$OUT\"",
        "pwd | cat >20",
        "pwd | bash",
        "pwd | python <script.py",
        "pwd | cat <in <<'DATA'\nvalue\nDATA\n",
        "pwd | cat 0>out <<'DATA'\nvalue\nDATA\n",
    ] {
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
