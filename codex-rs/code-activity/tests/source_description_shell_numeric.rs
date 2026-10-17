//! Inert developer regressions for C's direct numeric argv grammar.

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
        "authored-shell-numeric".into(),
        Language::Shell,
        source,
        AnalysisLimits::default(),
    )?;
    Ok(Analyzer::new()?.describe_complete(input))
}

#[test]
fn ordinary_numeric_argv_preserves_spelling_without_new_reference_rows() -> Result<(), ActivityError>
{
    for (executable, source) in [
        ("head", "head -n 40 notes.txt"),
        ("tail", "tail -n -40 notes.txt"),
        ("head", "head -00040 notes.txt"),
        ("tail", "tail -n -0 notes.txt"),
        ("head", "head -n +0040 notes.txt"),
        (
            "head",
            "head -n 999999999999999999999999999999999999999999 notes.txt",
        ),
        ("head", "head -n '0040' notes.txt"),
    ] {
        let result = describe(source)?;
        assert_eq!(
            result.disposition(),
            SourceDescriptionDisposition::CompleteSyntax,
            "{source}"
        );
        assert_eq!(result.sources().len(), 1, "{source}");
        assert_eq!(
            result.source_texts().first().map(String::as_str),
            Some(source),
            "{source}"
        );
        assert_eq!(result.references().len(), 1, "{source}");
        let invocation = result
            .references()
            .first()
            .ok_or(ActivityError::SourceLimit)?;
        assert_eq!(invocation.kind, SourceReferenceKind::InvocationCall);
        assert_eq!(invocation.literal.as_deref(), Some(executable));
        assert_eq!(
            source.get(invocation.evidence.start_byte..invocation.evidence.end_byte),
            Some(source)
        );
    }
    Ok(())
}

#[test]
fn uv_version_number_preserves_the_existing_empty_owned_child() -> Result<(), ActivityError> {
    for source in [
        "uv --python 3 run python -c ''",
        "uv run --python 3 python -c ''",
        "uv --python '3' run python -c ''",
    ] {
        let result = describe(source)?;
        assert_eq!(
            result.disposition(),
            SourceDescriptionDisposition::CompleteSyntax,
            "{source}"
        );
        assert_eq!(result.references().len(), 1, "{source}");
        assert_eq!(
            result
                .references()
                .first()
                .and_then(|reference| reference.literal.as_deref()),
            Some("uv")
        );
        assert_eq!(result.sources().len(), 2, "{source}");
        assert_eq!(result.source_texts().get(1).map(String::as_str), Some(""));
        let child = result.sources().get(1).ok_or(ActivityError::SourceLimit)?;
        assert_eq!(child.language, Language::Python);
        let parent = child.parent.as_ref().ok_or(ActivityError::SourceLimit)?;
        assert_eq!(source.get(parent.start_byte..parent.end_byte), Some("''"));
    }
    Ok(())
}

#[test]
fn composition_and_env_keep_their_existing_reference_order() -> Result<(), ActivityError> {
    let source = "LANG=C env PAGER='' head -n 0040 | tail -n -2 > out";
    let result = describe(source)?;
    assert_eq!(
        result.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    let rows: Vec<_> = result
        .references()
        .iter()
        .map(|reference| (reference.kind, reference.literal.as_deref()))
        .collect();
    assert_eq!(
        rows,
        vec![
            (SourceReferenceKind::InvocationCall, Some("env")),
            (SourceReferenceKind::EnvironmentAssignment, Some("LANG=C")),
            (SourceReferenceKind::EnvironmentAssignment, Some("PAGER=")),
            (SourceReferenceKind::InvocationCall, Some("head")),
            (SourceReferenceKind::ShellConnector, Some("|")),
            (SourceReferenceKind::InvocationCall, Some("tail")),
            (SourceReferenceKind::ShellRedirection, Some("out")),
        ]
    );
    assert_eq!(
        result.source_texts().first().map(String::as_str),
        Some(source)
    );
    Ok(())
}

#[test]
fn numeric_arguments_do_not_widen_other_positions_or_salvage_siblings() -> Result<(), ActivityError>
{
    for tail in [
        "head -n 0x10",
        "tail -n -0x10",
        "head -n 16#A",
        "head -n 2#${COUNT}",
        "head -n 3'0'",
        "mystery 40",
        "head -n 40 && mystery",
        "head -n 40 | mystery 2",
        "40",
        "env NAME=x 40",
        "exec 40",
        "cd 40",
        "cd -- 40",
        "env NAME=x cd 40",
        "COUNT=40 head -n 40",
        "head -n 40 > 20",
        "python -c 40",
        "node -e 40",
        "bash -c 40",
        "deno eval 40",
        "uv --python 3 run python -c 40",
        "python -c '' 40",
        "bash -c 'pwd' -- 40",
        "uv --python 7 run python -c ''",
    ] {
        let source = format!("git status; {tail}");
        let result = describe(&source)?;
        assert_eq!(
            result.disposition(),
            SourceDescriptionDisposition::Fallback(SourceDescriptionFallback::UnsupportedSource),
            "{source}"
        );
        assert!(result.sources().is_empty(), "{source}");
        assert!(result.source_texts().is_empty(), "{source}");
        assert!(result.references().is_empty(), "{source}");
        assert!(result.patches().is_empty(), "{source}");
    }
    Ok(())
}
