//! Development regressions for bounded literal filesystem member resolution.

use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::CompleteScript;
use codex_code_activity::Effect;
use codex_code_activity::EnvironmentAssumptions;
use codex_code_activity::Language;
use codex_code_activity::PresentationDisposition;
use codex_code_activity::Request;
use codex_code_activity::Target;
use codex_code_activity::WholeScriptActivity;
use pretty_assertions::assert_eq;
use std::time::Duration;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn strict(source: &str) -> TestResult<WholeScriptActivity> {
    let input = CompleteScript::from_request(
        Request::new(
            "computed-development".into(),
            Language::TypeScript,
            source.into(),
            None,
        ),
        EnvironmentAssumptions::FreshProcess,
        AnalysisLimits::new(32 * 1024, Duration::from_secs(5)),
    )?;
    Ok(Analyzer::new()?.analyze_complete(input))
}

#[test]
fn literal_keys_preserve_existing_read_write_and_edit_semantics() -> TestResult {
    for method in [
        "'readFileSync'",
        "('read' + 'FileSync')",
        "'readFile\\x53ync'",
    ] {
        let report = strict(&format!("require('fs')[{method}]('a','utf8');"))?;
        assert_eq!(report.disposition(), PresentationDisposition::Eligible);
        assert!(matches!(report.report().operations.as_slice(), [operation]
            if matches!(&operation.effect,
                Effect::FileRead { target: Target::Literal { path, .. } } if path == "a")));
    }
    let edit = strict(
        "const fs=require('fs');const s=fs['readFileSync']('a','utf8');fs['write'+'FileSync']('a',s.replace('old','new'));",
    )?;
    assert_eq!(edit.disposition(), PresentationDisposition::Eligible);
    assert_eq!(edit.ui_rows().len(), 2);
    assert!(edit.report().operations.iter().any(|operation| matches!(
        &operation.effect, Effect::FileEdit { target: Target::Literal { path, .. }, .. }
            if path == "a")));
    assert!(edit.diffs().is_empty());
    Ok(())
}

#[test]
fn known_module_and_callable_aliases_keep_source_order() -> TestResult {
    let report = strict(
        "const fs=require('fs');const other=fs;const write=other['writeFileSync'];write('a','new');other['appendFileSync']('a','tail');other['un'+'linkSync']('b');",
    )?;
    assert_eq!(report.disposition(), PresentationDisposition::Eligible);
    assert!(
        matches!(report.report().operations.as_slice(), [write, append, delete]
        if matches!(&write.effect, Effect::FileWrite { .. })
        && matches!(&append.effect, Effect::FileWrite { .. })
        && matches!(&delete.effect, Effect::FileDelete { .. }))
    );
    Ok(())
}

#[test]
fn computed_lookup_does_not_expand_the_diagnostic_baseline() -> TestResult {
    let source = "require('fs')['un'+'linkSync']('a')";
    let baseline = Analyzer::new()?.analyze(Request::new(
        "legacy".into(),
        Language::TypeScript,
        source.into(),
        None,
    ));
    assert!(baseline.operations.is_empty());
    assert!(!baseline.unresolved.is_empty());
    assert_eq!(
        strict(source)?.disposition(),
        PresentationDisposition::Eligible
    );
    Ok(())
}

#[test]
fn dynamic_keys_mutations_callbacks_and_unknown_suffixes_discard_every_row() -> TestResult {
    for source in [
        "const fs=require('fs');const k='readFileSync';fs[k]('a');",
        "const fs=require('fs');function key(){return 'readFileSync'}fs[key()]('a');",
        "const fs=require('fs');fs[`${'readFileSync'}`]('a');",
        "const fs=require('fs');fs['constructor']('return 1');",
        "const fs=require('fs');fs['promises']['readFile']('a');",
        "const fs=require('fs');fs['writeFileSync']=function(){};fs['writeFileSync']('a','x');",
        "const fs=require('fs');fs['writeFileSync']('a','x',function(){});",
        "const fs=require('fs');fs['readFileSync']('a',{flag:'w'});",
        "const fs=require('foreign-module');fs['readFileSync']('a');",
        "const fs=require('fs');fs['readFileSync']('a');uninspected();",
        "const fs=require('fs');function f(fs){fs['readFileSync']('a')}f({});",
        "let fs=require('fs');fs={};fs['readFileSync']('a');",
    ] {
        let report = strict(source)?;
        assert!(
            matches!(report.disposition(), PresentationDisposition::Fallback(_)),
            "{source}"
        );
        assert!(report.ui_rows().is_empty());
        assert!(report.diffs().is_empty());
    }
    Ok(())
}
