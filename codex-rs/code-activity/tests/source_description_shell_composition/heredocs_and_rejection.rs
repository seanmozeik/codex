use super::ActivityError;
use super::SourceDescriptionDisposition;
use super::SourceDescriptionFallback;
use super::SourceReferenceKind;
use super::assert_eq;
use super::describe;
use super::rows;

#[test]
fn cat_data_heredoc_keeps_all_physical_bytes_and_no_child_source() -> Result<(), ActivityError> {
    let description = describe("cat <<'DATA' > out\n  α\n\nDATA\n")?;
    assert_eq!(
        description.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    assert_eq!(description.sources().len(), 1);
    assert_eq!(
        rows(&description),
        vec![
            (SourceReferenceKind::InvocationCall, Some("cat"), "cat"),
            (SourceReferenceKind::ShellRedirection, Some("out"), "> out"),
            (SourceReferenceKind::HeredocData, None, "  α\n\n"),
        ]
    );
    let data = description
        .references()
        .last()
        .ok_or(ActivityError::SourceLimit)?;
    assert_eq!(
        (
            data.evidence.start_line,
            data.evidence.end_line,
            data.evidence.start_column
        ),
        (2, 4, 1)
    );
    Ok(())
}

#[test]
fn empty_and_whitespace_only_heredocs_retain_the_full_interval() -> Result<(), ActivityError> {
    for body in ["", " \t\n", "\n\n", "\t  "] {
        // The last case gains its required line break before the terminator.
        let body = if body.is_empty() || body.ends_with('\n') {
            body.to_owned()
        } else {
            format!("{body}\n")
        };
        let source = format!("cat <<'DATA'\n{body}DATA\n");
        let description = describe(&source)?;
        assert_eq!(
            description.disposition(),
            SourceDescriptionDisposition::CompleteSyntax,
            "{source:?}"
        );
        let data = description
            .references()
            .last()
            .ok_or(ActivityError::SourceLimit)?;
        assert_eq!(data.kind, SourceReferenceKind::HeredocData);
        assert_eq!(
            source.get(data.evidence.start_byte..data.evidence.end_byte),
            Some(body.as_str())
        );
        assert_eq!(data.evidence.start_byte, "cat <<'DATA'\n".len());
        assert_eq!(data.evidence.end_byte, "cat <<'DATA'\n".len() + body.len());
    }
    let empty_code = describe("python <<'PY'\nPY\n")?;
    assert_eq!(
        empty_code.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    assert_eq!(
        empty_code.source_texts().get(1).map(String::as_str),
        Some("")
    );
    let parent = empty_code
        .sources()
        .get(1)
        .and_then(|source| source.parent.as_ref())
        .ok_or(ActivityError::SourceLimit)?;
    assert_eq!(parent.start_byte, parent.end_byte);
    Ok(())
}

#[test]
fn owned_heredoc_suffix_keeps_parent_bytes_and_shell_before_child_order()
-> Result<(), ActivityError> {
    let source = "python 0<<'PY' | wc -l\n \t\nopen('a').read()\nPY\npwd";
    let description = describe(source)?;
    assert_eq!(
        description.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    assert_eq!(description.sources().len(), 2);
    assert_eq!(
        description.source_texts().get(1).map(String::as_str),
        Some(" \t\nopen('a').read()\n")
    );
    let parent = description
        .sources()
        .get(1)
        .and_then(|source| source.parent.as_ref())
        .ok_or(ActivityError::SourceLimit)?;
    assert_eq!(
        source.get(parent.start_byte..parent.end_byte),
        Some(" \t\nopen('a').read()\n")
    );
    assert_eq!(
        description
            .references()
            .iter()
            .take(4)
            .map(|reference| (
                reference.kind,
                reference.literal.as_deref(),
                reference.evidence.source_id.index(),
            ))
            .collect::<Vec<_>>(),
        vec![
            (SourceReferenceKind::InvocationCall, Some("python"), 0),
            (SourceReferenceKind::ShellConnector, Some("|"), 0),
            (SourceReferenceKind::InvocationCall, Some("wc"), 0),
            (SourceReferenceKind::InvocationCall, Some("pwd"), 0),
        ]
    );
    Ok(())
}

#[test]
fn conditional_heredoc_suffix_accepts_one_complete_atom_only() -> Result<(), ActivityError> {
    for operator in ["&&", "||"] {
        let source = format!("cat <<'DATA' {operator} git status > out\nliteral $HOME\nDATA\n");
        let description = describe(&source)?;
        assert_eq!(
            description.disposition(),
            SourceDescriptionDisposition::CompleteSyntax,
            "{source}"
        );
        assert_eq!(description.sources().len(), 1);
        assert_eq!(
            description
                .references()
                .iter()
                .map(|reference| reference.kind)
                .collect::<Vec<_>>(),
            vec![
                SourceReferenceKind::InvocationCall,
                SourceReferenceKind::HeredocData,
                SourceReferenceKind::ShellConnector,
                SourceReferenceKind::InvocationCall,
                SourceReferenceKind::ShellRedirection,
            ]
        );
    }
    Ok(())
}

#[test]
fn unsupported_composition_or_input_discards_every_prior_reference() -> Result<(), ActivityError> {
    for tail in [
        "git status && mystery",
        "mystery || git status",
        "git diff | mystery | wc -l",
        "(git status; mystery)",
        "{ git status; mystery; }",
        "{ git status\n}",
        "git status &",
        "git status |& wc -l",
        "! git status",
        "((1 + 2))",
        "(git status) > out",
        "git diff > one two",
        "git diff > $OUT",
        "git diff 3> out",
        "git diff &> out",
        "git diff >| out",
        "git diff 2>&-",
        "git diff 2>&'1'",
        "git diff 2>&1-",
        "cd",
        "cd -",
        "cd -- -",
        "cd -folder",
        "cd a b",
        "cd $DIR",
        "/bin/cd a",
        "NAME+=x git status",
        "NAME=(x) git status",
        "NAME=x NAME=y git status",
        "NAME=x env NAME=y git status",
        "NAME=x",
        "NAME=$VALUE git status",
        "env NAME=x",
        "env NAME=x command python -c ''",
        "env NAME=x env python -c ''",
        "env NAME=x exec python -c ''",
        "/bin/env NAME=x python -c ''",
        "env -- NAME=x python -c ''",
        "env --chdir=x python -c ''",
        "printf 'code' | python",
        "python < code.py",
        "python script.py",
        "cat <<DATA\n$HOME\nDATA\n",
        "cat <<-'DATA'\ntext\nDATA\n",
        "cat <<'DATA'\r\ntext\nDATA\n",
        "cat <<'DATA'\ntext\nDATA\r\n",
        "cat <<'DATA'\ntext\n DATA\n",
        "cat <<'DATA'\ntext\nDATAextra\n",
        "echo <<'DATA'\ntext\nDATA\n",
        "python -c '' <<'PY'\npass\nPY\n",
        "python <<'PY' < file\npass\nPY\n",
        "python <<'PY' 0> file\npass\nPY\n",
        "python <<'PY' <&1\npass\nPY\n",
        "python <<'PY' | python\npass\nPY\n",
        "cat <<'DATA' | (wc -l)\ntext\nDATA\n",
        "cat <<'DATA' | wc -l | cat\ntext\nDATA\n",
        "cat <<'DATA' && git status || pwd\ntext\nDATA\n",
        "cat <<'DATA' && mystery\ntext\nDATA\n",
    ] {
        let source = format!("git status; {tail}");
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
