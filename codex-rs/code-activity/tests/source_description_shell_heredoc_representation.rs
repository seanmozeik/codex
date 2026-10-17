//! Regressions for exact pinned-CST reconciliation; source is never executed.

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
        "authored-shell-heredoc-representation".into(),
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
fn hidden_header_lf_preserves_empty_whitespace_and_utf8_data() -> Result<(), ActivityError> {
    for tail in ["", " \t", " # exact header comment"] {
        for body in ["", "\n", " \t\n\n", "  α\n\n"] {
            let header = format!("cat <<'DATA'{tail}\n");
            let source = format!("{header}{body}DATA\n");
            let description = describe(&source)?;
            assert_eq!(
                description.disposition(),
                SourceDescriptionDisposition::CompleteSyntax
            );
            assert_eq!(description.source_texts(), std::slice::from_ref(&source));
            assert_eq!(description.sources().len(), 1);
            assert!(description.patches().is_empty());
            assert_eq!(
                rows(&description),
                vec![
                    (SourceReferenceKind::InvocationCall, Some("cat"), 0, "cat"),
                    (SourceReferenceKind::HeredocData, None, 0, body),
                ]
            );
            let data = &description.references()[1].evidence;
            assert_eq!(data.start_byte, header.len());
            assert_eq!(data.end_byte, header.len() + body.len());
            assert_eq!(data.start_line, 2);
            assert_eq!(data.start_column, 1);
            assert_eq!(
                data.end_line,
                2 + body.bytes().filter(|byte| *byte == b'\n').count()
            );
        }
    }
    Ok(())
}

#[test]
fn adjacent_zero_is_owned_by_its_redirect_and_excluded_from_argv() -> Result<(), ActivityError> {
    for (redirect, literal) in [
        ("0<'in'", Some("in")),
        ("0>'out'", Some("out")),
        ("0>>'out'", Some("out")),
        ("0<&0", None),
        ("0<&2", None),
        ("0>&0", None),
        ("0>&1", None),
    ] {
        let source = format!("cat {redirect}");
        let description = describe(&source)?;
        assert_eq!(
            description.disposition(),
            SourceDescriptionDisposition::CompleteSyntax
        );
        assert_eq!(description.source_texts(), std::slice::from_ref(&source));
        assert_eq!(
            rows(&description),
            vec![
                (SourceReferenceKind::InvocationCall, Some("cat"), 0, "cat"),
                (SourceReferenceKind::ShellRedirection, literal, 0, redirect),
            ]
        );
        assert_eq!(description.references()[0].evidence.end_byte, 3);
        assert_eq!(description.references()[1].evidence.start_byte, 4);
        assert_eq!(description.references()[1].evidence.end_byte, source.len());
    }
    Ok(())
}

#[test]
fn hidden_header_lf_retains_the_entire_owned_shell_child() -> Result<(), ActivityError> {
    let header = "bash <<'CODE'\n";
    let body = "\n  pwd\n";
    let source = format!("{header}{body}CODE\n");
    let description = describe(&source)?;
    assert_eq!(
        description.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    assert_eq!(
        description.source_texts(),
        &[source.clone(), body.to_owned()]
    );
    assert_eq!(
        rows(&description),
        vec![
            (SourceReferenceKind::InvocationCall, Some("bash"), 0, "bash"),
            (SourceReferenceKind::InvocationCall, Some("pwd"), 1, "pwd"),
        ]
    );
    let parent = description.sources()[1]
        .parent
        .as_ref()
        .ok_or(ActivityError::SourceLimit)?;
    assert_eq!(parent.source_id.index(), 0);
    assert_eq!(parent.start_byte, header.len());
    assert_eq!(parent.end_byte, header.len() + body.len());
    assert_eq!(source.get(parent.start_byte..parent.end_byte), Some(body));
    Ok(())
}

#[test]
fn a_quoted_zero_remains_an_ordinary_argument() -> Result<(), ActivityError> {
    let source = "cat '0'<'in'";
    let description = describe(source)?;
    assert_eq!(
        description.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    assert_eq!(
        rows(&description),
        vec![
            (
                SourceReferenceKind::InvocationCall,
                Some("cat"),
                0,
                "cat '0'"
            ),
            (
                SourceReferenceKind::ShellRedirection,
                Some("in"),
                0,
                "<'in'"
            ),
        ]
    );
    Ok(())
}

#[test]
fn a_whole_pipeline_wrapper_attaches_its_heredoc_only_to_the_final_atom()
-> Result<(), ActivityError> {
    let header = "echo input | cat | bash <<'CODE'\n";
    let body = "pwd\n";
    let source = format!("{header}{body}CODE\n");
    let description = describe(&source)?;
    assert_eq!(
        description.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    assert_eq!(description.source_texts(), &[source, body.to_owned()]);
    assert_eq!(
        rows(&description),
        vec![
            (
                SourceReferenceKind::InvocationCall,
                Some("echo"),
                0,
                "echo input"
            ),
            (SourceReferenceKind::ShellConnector, Some("|"), 0, "|"),
            (SourceReferenceKind::InvocationCall, Some("cat"), 0, "cat"),
            (SourceReferenceKind::ShellConnector, Some("|"), 0, "|"),
            (SourceReferenceKind::InvocationCall, Some("bash"), 0, "bash"),
            (SourceReferenceKind::InvocationCall, Some("pwd"), 1, "pwd"),
        ]
    );
    let parent = description.sources()[1]
        .parent
        .as_ref()
        .ok_or(ActivityError::SourceLimit)?;
    assert_eq!(parent.source_id.index(), 0);
    assert_eq!(parent.start_byte, header.len());
    assert_eq!(parent.end_byte, header.len() + body.len());
    Ok(())
}

#[test]
fn a_pipeline_data_heredoc_has_no_child_source() -> Result<(), ActivityError> {
    let source = "echo input | cat <<'DATA'\n  α\n\nDATA\n";
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
            (
                SourceReferenceKind::InvocationCall,
                Some("echo"),
                0,
                "echo input"
            ),
            (SourceReferenceKind::ShellConnector, Some("|"), 0, "|"),
            (SourceReferenceKind::InvocationCall, Some("cat"), 0, "cat"),
            (SourceReferenceKind::HeredocData, None, 0, "  α\n\n"),
        ]
    );
    Ok(())
}

#[test]
fn reconstruction_never_salvages_unsupported_headers_or_siblings() -> Result<(), ActivityError> {
    for tail in [
        "cat <<'E'\r\ndata\nE\n",
        "cat <<'E'\ndata\nE extra\n",
        "cat <<'E' arbitrary\ndata\nE\n",
        "cat 00<'in'",
        "cat 12<'in'",
        "cat 3<'in'",
        "cat 0>out <<'E'\ndata\nE\n",
        "cat <<'E'\ndata\nE\nunknown_leaf",
        "bash -c 'pwd' 0<<'E'\npwd\nE\n",
        "bash 0<<'E'\npwd; unknown_leaf\nE\n",
        "unknown_leaf | bash <<'E'\npwd\nE\n",
        "echo input | (bash -c 'pwd') <<'E'\npwd\nE\n",
        "(pwd) <<'E'\npwd\nE\n",
    ] {
        let source = format!("pwd\n{tail}");
        let description = describe(&source)?;
        assert_eq!(
            description.disposition(),
            SourceDescriptionDisposition::Fallback(SourceDescriptionFallback::UnsupportedSource),
            "{source:?}"
        );
        assert!(description.sources().is_empty());
        assert!(description.source_texts().is_empty());
        assert!(description.references().is_empty());
        assert!(description.patches().is_empty());
    }
    Ok(())
}
