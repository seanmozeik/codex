//! Path-call origins can change locally without changing prior alias origins.

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

fn describe(source: &str, language: Language) -> Result<SourceDescription, ActivityError> {
    let input = CompleteSourceDescriptionInput::from_source(
        "path-rebinding".into(),
        language,
        source,
        AnalysisLimits::default(),
    )?;
    Ok(Analyzer::new()?.describe_complete(input))
}

#[test]
fn sequential_paths_keep_each_read_and_write_reference() -> Result<(), ActivityError> {
    let source = "from pathlib import Path\np = Path('first')\np.write_text(p.read_text().replace('old','new',1))\np = Path('second')\np.write_text(p.read_text() + '\\n')";
    let result = describe(source, Language::Python)?;
    assert_eq!(
        result.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    let references: Vec<_> = result
        .references()
        .iter()
        .filter(|reference| {
            matches!(
                reference.kind,
                SourceReferenceKind::ReadCall | SourceReferenceKind::WriteCall
            )
        })
        .map(|reference| (reference.kind, reference.literal.as_deref()))
        .collect();
    assert_eq!(
        references,
        vec![
            (SourceReferenceKind::ReadCall, Some("first")),
            (SourceReferenceKind::WriteCall, Some("first")),
            (SourceReferenceKind::ReadCall, Some("second")),
            (SourceReferenceKind::WriteCall, Some("second")),
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
fn previous_path_alias_remains_bound_to_its_original_literal() -> Result<(), ActivityError> {
    let source = "from pathlib import Path\np = Path('old/./name')\nprevious = p\nnext = Path('new')\np = next\np.write_text(p.read_text())\nprevious.write_text(previous.read_text())";
    let result = describe(source, Language::Python)?;
    assert_eq!(
        result.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    assert_eq!(
        result
            .references()
            .iter()
            .filter(|reference| reference.kind == SourceReferenceKind::ReadCall)
            .map(|reference| reference.literal.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("new"), Some("old/./name")]
    );
    Ok(())
}

#[test]
fn unsupported_path_rhs_clears_the_entire_recognized_prefix() -> Result<(), ActivityError> {
    for rhs in [
        "unknown()",
        "Path(dynamic)",
        "Path('a' + 'b')",
        "Path(p.read_text())",
        "Path('b', 'c')",
        "Path(path='b')",
        "Path(*parts)",
        "Path(**options)",
        "'data'",
        "p.read_text()",
        "open",
        "open('b')",
        "print('side effect')",
    ] {
        let source = format!("from pathlib import Path\np = Path('a')\np.read_text()\np = {rhs}");
        let result = describe(&source, Language::Python)?;
        assert!(
            matches!(
                result.disposition(),
                SourceDescriptionDisposition::Fallback(_)
            ),
            "{rhs}"
        );
        assert!(result.references().is_empty(), "{rhs}");
        assert!(result.sources().is_empty(), "{rhs}");
    }
    Ok(())
}

#[test]
fn constructor_callable_handle_and_data_origins_do_not_gain_type_changes()
-> Result<(), ActivityError> {
    for body in [
        "Path = Path('a')",
        "Path = 'shadow'",
        "p = open\np = Path('a')",
        "p = open('a')\np = Path('b')",
        "p = 'a'\np = Path('a')",
        "p = open('a').read()\np = Path('b')",
        "p = 'a'.replace('a','b')\np = Path('b')",
        "p = Path('a')\np.attribute = Path('b')",
        "p = Path('a')\np['key'] = Path('b')",
        "p = Path('a')\ndel p",
        "open = Path('a')",
    ] {
        let result = describe(
            &format!("from pathlib import Path\n{body}"),
            Language::Python,
        )?;
        assert!(
            matches!(
                result.disposition(),
                SourceDescriptionDisposition::Fallback(_)
            ),
            "{body}"
        );
        assert!(result.references().is_empty(), "{body}");
    }
    Ok(())
}

#[test]
fn repeated_path_assignments_never_refund_cumulative_storage() -> Result<(), ActivityError> {
    let source = format!(
        "from pathlib import Path\nname = '{}'\np = Path(name)\n{}",
        "x".repeat(16_000),
        "p = Path(name)\n".repeat(35)
    );
    let result = describe(&source, Language::Python)?;
    assert_eq!(
        result.disposition(),
        SourceDescriptionDisposition::Fallback(SourceDescriptionFallback::SourceLimit)
    );
    assert!(result.references().is_empty());
    Ok(())
}

#[test]
fn bad_shell_sibling_clears_a_supported_path_rebinding_child() -> Result<(), ActivityError> {
    let child =
        "from pathlib import Path; p = Path('a'); p.read_text(); p = Path('b'); p.write_text('x')";
    assert_eq!(
        describe(child, Language::Python)?.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    for source in [
        format!("unknown before; python -c \"{child}\""),
        format!("python -c \"{child}\"; unknown after"),
    ] {
        let result = describe(&source, Language::Shell)?;
        assert_eq!(
            result.disposition(),
            SourceDescriptionDisposition::Fallback(SourceDescriptionFallback::UnsupportedSource)
        );
        assert!(result.references().is_empty());
        assert!(result.sources().is_empty());
    }
    Ok(())
}

#[test]
fn path_rebinding_does_not_change_unknown_runtime_effect_contract() -> Result<(), ActivityError> {
    let source =
        "from pathlib import Path\np = Path('a')\np.read_text()\np = Path('b')\np.write_text('x')";
    assert_eq!(
        describe(source, Language::Python)?.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    let input = CompleteScript::from_request(
        Request::new("unknown".into(), Language::Python, source.into(), None),
        EnvironmentAssumptions::PersistentOrUnknown,
        AnalysisLimits::default(),
    )?;
    let result = Analyzer::new()?.analyze_complete(input);
    assert!(matches!(
        result.disposition(),
        PresentationDisposition::Fallback(_)
    ));
    assert!(result.ui_rows().is_empty());
    Ok(())
}
