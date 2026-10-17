//! Explicit trusted-pathlib profile boundaries and whole-source fallback.

use codex_code_activity::ActivityError;
use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::CompleteScript;
use codex_code_activity::EnvironmentAssumptions;
use codex_code_activity::FileSnapshot;
use codex_code_activity::Language;
use codex_code_activity::PresentationDisposition;
use codex_code_activity::PythonRuntimeProfile;
use codex_code_activity::Request;
use codex_code_activity::Target;
use codex_code_activity::WholeScriptActivity;
use pretty_assertions::assert_eq;
use serde::Deserialize;

#[derive(Deserialize)]
struct Case {
    id: String,
    source: String,
    expected: Option<Vec<String>>,
    expected_operations: Option<Vec<serde_json::Value>>,
}

fn analyze(source: &str) -> Result<WholeScriptActivity, ActivityError> {
    let request = Request::new(
        "authored".into(),
        Language::Python,
        source.into(),
        Some("/work".into()),
    )
    .with_snapshots(vec![FileSnapshot::new(
        "a".into(),
        Some("old".into()),
        Some("new".into()),
    )]);
    let script = CompleteScript::from_request(
        request,
        EnvironmentAssumptions::FreshProcess,
        AnalysisLimits::default(),
    )?
    .with_python_runtime_profile(PythonRuntimeProfile::PreloadedTrustedPathlib);
    Ok(Analyzer::new()?.analyze_complete(script))
}

#[test]
fn authored_development_contract() -> Result<(), Box<dyn std::error::Error>> {
    let cases: Vec<Case> = serde_json::from_str(include_str!("fixtures/trusted_pathlib.json"))?;
    for case in cases {
        let activity = analyze(&case.source)?;
        if let Some(expected) = case.expected {
            assert_eq!(
                activity.disposition(),
                PresentationDisposition::Eligible,
                "{}",
                case.id
            );
            let report = serde_json::to_value(activity.report())?;
            let operations = report["operations"]
                .as_array()
                .ok_or("serialized report operations must be an array")?;
            let actual: Vec<_> = operations
                .iter()
                .map(|operation| {
                    operation["effect"]["type"]
                        .as_str()
                        .map(str::to_owned)
                        .ok_or("serialized effect must have a string type")
                })
                .collect::<Result<_, _>>()?;
            assert_eq!(actual, expected, "{}", case.id);
            let effects: Vec<_> = operations
                .iter()
                .map(|operation| operation["effect"].clone())
                .collect();
            let expected_effects = case
                .expected_operations
                .ok_or("eligible fixture must include exact expected effects")?;
            assert_eq!(effects, expected_effects, "{}", case.id);
        } else {
            assert!(
                matches!(activity.disposition(), PresentationDisposition::Fallback(_)),
                "{}",
                case.id
            );
            assert!(activity.report().operations.is_empty(), "{}", case.id);
            assert!(activity.ui_rows().is_empty(), "{}", case.id);
            assert!(activity.diffs().is_empty(), "{}", case.id);
        }
    }
    Ok(())
}

#[test]
fn default_environment_and_wrong_language_do_not_gain_import_trust() -> Result<(), ActivityError> {
    for (language, environment, profile) in [
        (
            Language::Python,
            EnvironmentAssumptions::FreshProcess,
            PythonRuntimeProfile::Default,
        ),
        (
            Language::Python,
            EnvironmentAssumptions::PersistentOrUnknown,
            PythonRuntimeProfile::PreloadedTrustedPathlib,
        ),
        (
            Language::Shell,
            EnvironmentAssumptions::FreshProcess,
            PythonRuntimeProfile::PreloadedTrustedPathlib,
        ),
        (
            Language::TypeScript,
            EnvironmentAssumptions::FreshProcess,
            PythonRuntimeProfile::PreloadedTrustedPathlib,
        ),
    ] {
        let source = "from pathlib import Path\nPath('a').write_text('x')";
        let script = CompleteScript::from_request(
            Request::new("boundary".into(), language, source.into(), None),
            environment,
            AnalysisLimits::default(),
        )?
        .with_python_runtime_profile(profile);
        let activity = Analyzer::new()?.analyze_complete(script);
        assert!(matches!(
            activity.disposition(),
            PresentationDisposition::Fallback(_)
        ));
        assert!(activity.ui_rows().is_empty());
    }
    Ok(())
}

#[test]
fn targets_follow_posix_spelling_without_resolving_parent_components() -> Result<(), ActivityError>
{
    let activity = analyze("from pathlib import Path\nPath('//a/./b//../c/').read_text()")?;
    let codex_code_activity::Effect::FileRead { target } = &activity.report().operations[0].effect
    else {
        panic!("expected read");
    };
    assert_eq!(
        target,
        &Target::Literal {
            path: "//a/b/../c".into(),
            cwd: Some("/work".into())
        }
    );
    Ok(())
}

#[test]
fn default_and_partial_paths_remain_available_after_profile_analysis() -> Result<(), ActivityError>
{
    let mut analyzer = Analyzer::new()?;
    let script = CompleteScript::from_request(
        Request::new(
            "profile".into(),
            Language::Python,
            "from pathlib import Path\nPath('a').write_text('x')".into(),
            None,
        ),
        EnvironmentAssumptions::FreshProcess,
        AnalysisLimits::default(),
    )?
    .with_python_runtime_profile(PythonRuntimeProfile::PreloadedTrustedPathlib);
    assert_eq!(
        analyzer.analyze_complete(script).disposition(),
        PresentationDisposition::Eligible
    );
    let request = || {
        Request::new(
            "ordinary".into(),
            Language::Python,
            "from pathlib import Path\nPath('a').read_text()".into(),
            None,
        )
    };
    let baseline = analyzer.analyze(request());
    assert_eq!(baseline.operations.len(), 1);
    assert!(!baseline.unresolved.is_empty());
    let ordinary = CompleteScript::from_request(
        request(),
        EnvironmentAssumptions::FreshProcess,
        AnalysisLimits::default(),
    )?;
    assert!(matches!(
        analyzer.analyze_complete(ordinary).disposition(),
        PresentationDisposition::Fallback(_)
    ));
    Ok(())
}

#[test]
fn physical_nul_in_comment_abstains_but_escaped_nul_text_is_supported() -> Result<(), ActivityError>
{
    let invalid = analyze("from pathlib import Path\n# \0\nPath('a').write_text('x')")?;
    assert!(matches!(
        invalid.disposition(),
        PresentationDisposition::Fallback(_)
    ));
    let valid = analyze("from pathlib import Path\nPath('a').write_text('\\x00')")?;
    assert_eq!(valid.disposition(), PresentationDisposition::Eligible);
    Ok(())
}
