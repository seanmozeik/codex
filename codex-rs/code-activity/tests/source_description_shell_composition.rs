//! Developer regressions for the frozen finite shell-composition grammar.

#[path = "source_description_shell_composition/heredocs_and_rejection.rs"]
mod heredocs_and_rejection;

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
        "authored-shell-composition".into(),
        Language::Shell,
        source,
        AnalysisLimits::default(),
    )?;
    Ok(Analyzer::new()?.describe_complete(input))
}

fn rows(description: &SourceDescription) -> Vec<(SourceReferenceKind, Option<&str>, &str)> {
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
                &source[evidence.start_byte..evidence.end_byte],
            )
        })
        .collect()
}

#[test]
fn conditionals_and_pipelines_keep_connector_and_invocation_order() -> Result<(), ActivityError> {
    let description = describe("git status && cargo test | wc -l")?;
    assert_eq!(
        description.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    assert_eq!(
        rows(&description),
        vec![
            (
                SourceReferenceKind::InvocationCall,
                Some("git"),
                "git status"
            ),
            (SourceReferenceKind::ShellConnector, Some("&&"), "&&"),
            (
                SourceReferenceKind::InvocationCall,
                Some("cargo"),
                "cargo test"
            ),
            (SourceReferenceKind::ShellConnector, Some("|"), "|"),
            (SourceReferenceKind::InvocationCall, Some("wc"), "wc -l"),
        ]
    );
    Ok(())
}

#[test]
fn groups_and_cd_preserve_exact_unicode_operand_without_resolving_it() -> Result<(), ActivityError>
{
    let description = describe("(pwd); { cd -- 'δ'; cat 'a'; }")?;
    assert_eq!(
        description.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    assert_eq!(
        rows(&description),
        vec![
            (SourceReferenceKind::ShellConnector, Some("("), "("),
            (SourceReferenceKind::InvocationCall, Some("pwd"), "pwd"),
            (SourceReferenceKind::ShellConnector, Some(")"), ")"),
            (SourceReferenceKind::ShellConnector, Some("{"), "{"),
            (SourceReferenceKind::InvocationCall, Some("cd"), "cd -- 'δ'"),
            (SourceReferenceKind::DirectoryOperand, Some("δ"), "'δ'"),
            (SourceReferenceKind::InvocationCall, Some("cat"), "cat 'a'"),
            (SourceReferenceKind::ShellConnector, Some("}"), "}"),
        ]
    );
    Ok(())
}

#[test]
fn redirects_distinguish_cooked_paths_from_descriptor_destinations() -> Result<(), ActivityError> {
    let description = describe("git diff > 'δ.diff' 2>&1")?;
    assert_eq!(
        description.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    assert_eq!(
        rows(&description),
        vec![
            (SourceReferenceKind::InvocationCall, Some("git"), "git diff"),
            (
                SourceReferenceKind::ShellRedirection,
                Some("δ.diff"),
                "> 'δ.diff'"
            ),
            (SourceReferenceKind::ShellRedirection, None, "2>&1"),
        ]
    );
    Ok(())
}

#[test]
fn prefix_and_env_assignments_have_a_distinct_inner_invocation() -> Result<(), ActivityError> {
    let source = "LANG=C env PAGER='' cd -- 'δ'";
    let description = describe(source)?;
    assert_eq!(
        description.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    assert_eq!(
        rows(&description),
        vec![
            (SourceReferenceKind::InvocationCall, Some("env"), source),
            (
                SourceReferenceKind::EnvironmentAssignment,
                Some("LANG=C"),
                "LANG=C"
            ),
            (
                SourceReferenceKind::EnvironmentAssignment,
                Some("PAGER="),
                "PAGER=''"
            ),
            (SourceReferenceKind::InvocationCall, Some("cd"), "cd -- 'δ'"),
            (SourceReferenceKind::DirectoryOperand, Some("δ"), "'δ'"),
        ]
    );
    Ok(())
}

#[test]
fn static_environment_and_cd_spellings_accept_empty_literals() -> Result<(), ActivityError> {
    for source in [
        "NAME= git status",
        "NAME='a'\"b\" git status",
        "'en'v NAME=x git status",
        "env NAME= cd ''",
        "cd -- '-folder'",
        "LANG=C exec python -c \"open('a').read()\"",
        "env LANG=C Python -c \"open('a').read()\"",
    ] {
        let description = describe(source)?;
        assert_eq!(
            description.disposition(),
            SourceDescriptionDisposition::CompleteSyntax,
            "{source}"
        );
    }
    Ok(())
}
