//! Explicit stdin heredocs retain ownership without repairing an error tree.

use codex_code_activity::ActivityError;
use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::CompleteSourceDescriptionInput;
use codex_code_activity::Language;
use codex_code_activity::SourceDescription;
use codex_code_activity::SourceDescriptionDisposition;
use codex_code_activity::SourceDescriptionFallback;
use codex_code_activity::SourceReferenceKind;
use pretty_assertions::assert_eq;

fn describe(source: &str) -> Result<SourceDescription, ActivityError> {
    let input = CompleteSourceDescriptionInput::from_source(
        "authored-heredoc-descriptor".into(),
        Language::Shell,
        source,
        AnalysisLimits::default(),
    )?;
    Ok(Analyzer::new()?.describe_complete(input))
}

#[test]
fn explicit_stdin_retains_the_entire_data_body() -> Result<(), ActivityError> {
    for body in ["", " \t\n\n", " \t\nα\n\n"] {
        let header = "cat 0<<'DATA'\n";
        let source = format!("{header}{body}DATA\n");
        let description = describe(&source)?;
        assert_eq!(
            description.disposition(),
            SourceDescriptionDisposition::CompleteSyntax
        );
        assert_eq!(description.source_texts(), std::slice::from_ref(&source));
        let [invocation, data] = description.references() else {
            panic!("expected an invocation and owned data: {description:?}");
        };
        assert_eq!(invocation.kind, SourceReferenceKind::InvocationCall);
        assert_eq!(
            source.get(invocation.evidence.start_byte..invocation.evidence.end_byte),
            Some("cat")
        );
        assert_eq!(data.kind, SourceReferenceKind::HeredocData);
        assert_eq!(data.evidence.source_id.index(), 0);
        assert_eq!(data.evidence.start_byte, header.len());
        assert_eq!(data.evidence.end_byte, header.len() + body.len());
        assert_eq!(
            source.get(data.evidence.start_byte..data.evidence.end_byte),
            Some(body)
        );
    }
    Ok(())
}

#[test]
fn explicit_stdin_keeps_shell_children_and_parent_invocations_ordered() -> Result<(), ActivityError>
{
    for header in ["bash 0<<'CODE' | cat\n", "echo input | bash 0<<'CODE'\n"] {
        let body = "\n  pwd\n";
        let source = format!("{header}{body}CODE\npwd");
        let description = describe(&source)?;
        assert_eq!(
            description.disposition(),
            SourceDescriptionDisposition::CompleteSyntax
        );
        assert_eq!(
            description.source_texts(),
            &[source.clone(), body.to_owned()]
        );
        let parent = description.sources()[1]
            .parent
            .as_ref()
            .ok_or(ActivityError::SourceLimit)?;
        assert_eq!(parent.source_id.index(), 0);
        assert_eq!(parent.start_byte, header.len());
        assert_eq!(parent.end_byte, header.len() + body.len());
        assert_eq!(source.get(parent.start_byte..parent.end_byte), Some(body));
        let rows: Vec<_> = description
            .references()
            .iter()
            .map(|reference| {
                (
                    reference.literal.as_deref(),
                    reference.evidence.source_id.index(),
                )
            })
            .collect();
        let (first, last) = if header.starts_with("bash") {
            ("bash", "cat")
        } else {
            ("echo", "bash")
        };
        assert_eq!(
            rows,
            vec![
                (Some(first), 0),
                (Some("|"), 0),
                (Some(last), 0),
                (Some("pwd"), 0),
                (Some("pwd"), 1),
            ]
        );
    }
    Ok(())
}

#[test]
fn unsupported_descriptor_owners_and_suffixes_discard_the_whole_description()
-> Result<(), ActivityError> {
    for tail in [
        "cat 00<<'E'\ndata\nE\n",
        "cat 1<<'E'\ndata\nE\n",
        "cat 2<<'E'\ndata\nE\n",
        "cat 10<<'E'\ndata\nE\n",
        "cat 0<<'E' 0<other\ndata\nE\n",
        "cat 0<other 0<<'E'\ndata\nE\n",
        "cat 0<<'E' 0<&1\ndata\nE\n",
        "cat 0<<'E'\ndata\nE extra\n",
        "cat 0<<'E'\ndata\n",
        "cat 0<<'E'\ndata\nE\nunknown_leaf",
        "bash -c 'pwd' 0<<'E'\npwd\nE\n",
        "bash 0<<'E'\npwd; unknown_leaf\nE\n",
        "cat 0<<'E' | unknown_leaf\ndata\nE\n",
    ] {
        let source = format!("pwd\n{tail}");
        let description = describe(&source)?;
        assert_eq!(
            description.disposition(),
            SourceDescriptionDisposition::Fallback(SourceDescriptionFallback::UnsupportedSource),
            "{source:?}"
        );
        assert!(description.sources().is_empty(), "{source:?}");
        assert!(description.source_texts().is_empty(), "{source:?}");
        assert!(description.references().is_empty(), "{source:?}");
        assert!(description.patches().is_empty(), "{source:?}");
    }
    Ok(())
}
