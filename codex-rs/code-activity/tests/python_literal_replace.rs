//! Authored strict-only literal replacement cases; Python source stays inert.

use codex_code_activity::ActivityError;
use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::CompleteScript;
use codex_code_activity::Effect;
use codex_code_activity::EnvironmentAssumptions;
use codex_code_activity::FileSnapshot;
use codex_code_activity::Language;
use codex_code_activity::PresentationDisposition;
use codex_code_activity::Request;
use codex_code_activity::Target;
use codex_code_activity::WholeScriptActivity;
use pretty_assertions::assert_eq;
use std::time::Duration;

fn strict(source: &str) -> Result<WholeScriptActivity, ActivityError> {
    let input = CompleteScript::from_request(
        Request::new(
            "literal-development".into(),
            Language::Python,
            source.into(),
            None,
        )
        .with_snapshots(vec![FileSnapshot::new(
            "a".into(),
            Some("before".into()),
            Some("after".into()),
        )]),
        EnvironmentAssumptions::FreshProcess,
        AnalysisLimits::new(32 * 1024, Duration::from_secs(5)),
    )?;
    Ok(Analyzer::new()?.analyze_complete(input))
}

fn fallback(source: &str) -> Result<(), ActivityError> {
    let activity = strict(source)?;
    assert!(matches!(
        activity.disposition(),
        PresentationDisposition::Fallback(_)
    ));
    assert!(activity.report().operations.is_empty());
    assert!(activity.ui_rows().is_empty());
    assert!(activity.diffs().is_empty());
    Ok(())
}

#[test]
fn literal_paths_preserve_read_write_and_same_file_edit_intent() -> Result<(), ActivityError> {
    for source in [
        "p='draft.txt'.replace('draft','final')\nopen(p).read()",
        "rename='draft.txt'.replace\np=rename('draft','final')\nopen(p).read()",
        "p=('draft'+'.txt').replace('draft','final')\nopen(p).read()",
    ] {
        let activity = strict(source)?;
        assert_eq!(activity.disposition(), PresentationDisposition::Eligible);
        assert!(
            matches!(activity.report().operations.as_slice(), [operation]
            if matches!(&operation.effect, Effect::FileRead {
                target: Target::Literal { path, .. }
            } if path == "final.txt"))
        );
    }
    let write = strict("p='draft.txt'.replace('draft','final')\nopen(p,'w').write('hello')")?;
    assert_eq!(write.disposition(), PresentationDisposition::Eligible);
    assert!(write.report().operations.iter().any(|operation| matches!(
        &operation.effect, Effect::FileWrite { target: Target::Literal { path, .. }, .. }
            if path == "final.txt"
    )));
    let edit = strict(
        "p='draft.txt'.replace('draft','final')\ns=open(p).read()\nopen(p,'w').write(s.replace('old','new'))",
    )?;
    assert_eq!(edit.disposition(), PresentationDisposition::Eligible);
    assert!(edit.report().operations.iter().any(|operation| matches!(
        &operation.effect, Effect::FileEdit { target: Target::Literal { path, .. }, .. }
            if path == "final.txt"
    )));
    Ok(())
}

#[test]
fn only_two_ordinary_text_arguments_are_folded() -> Result<(), ActivityError> {
    for source in [
        "'a'.replace('a')",
        "'a'.replace('a','b',1)",
        "'a'.replace('a',1)",
        "'a'.replace(1,'b')",
        "'a'.replace('a',b'b')",
        "'a'.replace('a',unknown)",
        "'a'.replace(old='a',new='b')",
        "'a'.replace(*['a','b'])",
        "'a'.replace('a',**{'new':'b'})",
        "'a'.replace('a',callback())",
        "rename='a'.replace\nrename('a',unknown)",
    ] {
        fallback(source)?;
    }
    Ok(())
}

#[test]
fn unresolved_source_discards_every_known_operation_and_diff() -> Result<(), ActivityError> {
    for source in [
        "custom()\nopen('a'.replace('a','b')).read()",
        "open('a'.replace('a','b')).read()\ncustom()",
        "open('a'.replace('a','b')).read()\nif (",
        "import pathlib\nopen('a'.replace('a','b')).read()",
        "alias='a'.replace\nalias=unknown\nopen(alias('a','b')).read()",
        "open=unknown\nopen('a'.replace('a','b')).read()",
        "object={'replace':open}\nobject.replace('a','r',1)",
        "def replace(a,b,c):\n return a\nreplace('a','b',1)",
        "alias=open\nalias('a','r',1,'custom_codec').read()",
    ] {
        fallback(source)?;
    }
    Ok(())
}

#[test]
fn unused_oversized_fold_also_forces_whole_script_fallback() -> Result<(), ActivityError> {
    let input = "a".repeat(8_160);
    let accepted = strict(&format!("'{input}'.replace('a','xx')"))?;
    assert_eq!(accepted.disposition(), PresentationDisposition::Eligible);
    fallback(&format!(
        "open('known').read()\n'{input}a'.replace('a','xx')"
    ))?;
    let unicode = strict(&format!("'{input}'.replace('a','é')"))?;
    assert_eq!(unicode.disposition(), PresentationDisposition::Eligible);
    fallback(&format!("open('known').read()\n'{input}'.replace('','xx')"))?;
    Ok(())
}

#[test]
fn baseline_and_typescript_do_not_inherit_python_folding() -> Result<(), ActivityError> {
    let source = "open('a'.replace('a','b')).read()";
    let baseline = Analyzer::new()?.analyze(Request::new(
        "baseline".into(),
        Language::Python,
        source.into(),
        None,
    ));
    assert!(matches!(baseline.operations.as_slice(), [operation]
        if matches!(operation.effect, Effect::FileRead { target: Target::Unresolved })));
    let input = CompleteScript::from_request(
        Request::new(
            "javascript".into(),
            Language::TypeScript,
            "require('fs').readFileSync('a'.replace('a','b'))".into(),
            None,
        ),
        EnvironmentAssumptions::FreshProcess,
        AnalysisLimits::new(32 * 1024, Duration::from_secs(5)),
    )?;
    let activity = Analyzer::new()?.analyze_complete(input);
    assert!(matches!(
        activity.disposition(),
        PresentationDisposition::Fallback(_)
    ));
    assert!(activity.ui_rows().is_empty());
    Ok(())
}
