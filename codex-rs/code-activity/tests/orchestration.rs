//! Authored source-only orchestration contract; no analyzed command is executed.

use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::CompleteOrchestration;
use codex_code_activity::Effect;
use codex_code_activity::EnvironmentAssumptions;
use codex_code_activity::OrchestrationActivity;
use codex_code_activity::OrchestrationBuffer;
use codex_code_activity::OrchestrationDisposition;
use codex_code_activity::OrchestrationFallback;
use codex_code_activity::OrchestrationGroup;
use codex_code_activity::OrchestrationInputError;
use codex_code_activity::OrchestrationJoin;
use codex_code_activity::OrchestratorBindings;
use codex_code_activity::Target;
use pretty_assertions::assert_eq;
use std::time::Duration;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn limits() -> AnalysisLimits {
    AnalysisLimits::new(32 * 1024, Duration::from_secs(5))
}

fn complete(source: &str) -> Result<CompleteOrchestration, OrchestrationInputError> {
    CompleteOrchestration::from_source(
        "parent".into(),
        source,
        OrchestratorBindings::TrustedFreshHostBindings,
        EnvironmentAssumptions::FreshProcess,
        limits(),
    )
}

fn analyze(source: &str) -> TestResult<OrchestrationActivity> {
    Ok(Analyzer::new()?.analyze_orchestration(complete(source)?))
}

fn fallback(source: &str) -> TestResult {
    let activity = analyze(source)?;
    assert!(matches!(
        activity.disposition(),
        OrchestrationDisposition::Fallback(_)
    ));
    assert!(activity.groups().is_empty());
    assert_eq!(activity.source(), source);
    assert_eq!(activity.call_id().as_str(), "parent");
    Ok(())
}

#[test]
fn complete_python_carrier_and_original_sources_are_preserved() -> TestResult {
    let command =
        "python - <<'PY'\ns=open('note').read()\nopen('note','w').write(s.replace('a','b'))\nPY";
    let source = format!(
        "await tools.exec_command({{cmd:{}}});",
        serde_json::to_string(command)?
    );
    let activity = analyze(&source)?;
    assert_eq!(activity.disposition(), OrchestrationDisposition::Eligible);
    assert_eq!(activity.source(), source);
    let group = &activity.groups()[0];
    assert_eq!(group.join(), OrchestrationJoin::Single);
    let child = &group.children()[0];
    assert_eq!(child.command(), command);
    assert_eq!(child.id().index(), 0);
    assert_eq!(child.activity().report().call_id.as_str(), "parent:child:0");
    assert!(
        source[child.call_span().start_byte..child.call_span().end_byte]
            .starts_with("tools.exec_command(")
    );
    assert!(child.activity().diffs().is_empty());
    let operations = &child.activity().report().operations;
    assert!(matches!(operations.as_slice(), [read, open, edit]
        if matches!(&read.effect, Effect::FileRead { target: Target::Literal { path, .. } } if path == "note")
        && matches!(&open.effect, Effect::FileOpen { .. })
        && matches!(&edit.effect, Effect::FileEdit { target: Target::Literal { path, .. }, .. } if path == "note")));
    let serialized = serde_json::to_value(&activity)?;
    assert!(serialized.get("source").is_none());
    assert!(
        serialized["groups"][0]["children"][0]
            .get("command")
            .is_none()
    );
    Ok(())
}

#[test]
fn bindings_and_join_groups_keep_child_identity_without_completion_claims() -> TestResult {
    let source = r#"const a="cat first";const b=a;const result=await Promise.all([tools.exec_command({cmd:b}),tools.exec_command({cmd:"cat second"})]);await Promise.allSettled([tools.exec_command({cmd:"cat third"})]);"#;
    let activity = analyze(source)?;
    assert_eq!(activity.disposition(), OrchestrationDisposition::Eligible);
    assert_eq!(activity.groups().len(), 2);
    assert_eq!(activity.groups()[0].join(), OrchestrationJoin::All);
    assert_eq!(activity.groups()[1].join(), OrchestrationJoin::AllSettled);
    let children: Vec<_> = activity
        .groups()
        .iter()
        .flat_map(OrchestrationGroup::children)
        .collect();
    assert_eq!(
        children
            .iter()
            .map(|child| child.id().index())
            .collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    assert_eq!(
        children
            .iter()
            .map(|child| child.command())
            .collect::<Vec<_>>(),
        vec!["cat first", "cat second", "cat third"]
    );
    Ok(())
}

#[test]
fn unsupported_surrounding_or_child_source_discards_the_whole_envelope() -> TestResult {
    for source in [
        r#"await tools.exec_command({cmd:"cat a"});custom();"#,
        r#"custom();await tools.exec_command({cmd:"cat a"});"#,
        r#"await Promise.all([tools.exec_command({cmd:"cat a"}),tools.exec_command({cmd:"opaque_program"})]);"#,
        r#"await tools.exec_command({cmd:"cat a",login:false});"#,
        r#"let a="cat a";await tools.exec_command({cmd:a});"#,
        r#"const tools="x";await tools.exec_command({cmd:"cat a"});"#,
        r#"const Promise="x";await Promise.all([]);"#,
        r#"const eval="cat a";await tools.exec_command({cmd:eval});"#,
        r#"const arguments="cat a";await tools.exec_command({cmd:arguments});"#,
        r#"const await="cat a";await tools.exec_command({cmd:await});"#,
        r#"const yield="cat a";await tools.exec_command({cmd:yield});"#,
        r#"const implements="cat a";await tools.exec_command({cmd:implements});"#,
        r#"const a=await tools.exec_command({cmd:"cat a"});await tools.exec_command({cmd:a});"#,
        r#"const f=()=>tools.exec_command({cmd:"cat a"});"#,
        r#"await "Promise".all([tools.exec_command({cmd:"cat a"})]);"#,
        r#"await tools?.exec_command({cmd:"cat a"});"#,
        r#"await tools.exec_command?.({cmd:"cat a"});"#,
        r#"await tools.exec_command<string>({cmd:"cat a"});"#,
        r#"await Promise.all([,tools.exec_command({cmd:"cat a"})]);"#,
        r#"await Promise.all([...children]);"#,
        r#"await tools.exec_command({cmd:`cat a`});"#,
        r#"await tools.exec_command({cmd:"cat a"});if ("#,
    ] {
        fallback(source)?;
    }
    Ok(())
}

#[test]
fn js_escape_semantics_are_preserved_or_rejected() -> TestResult {
    let source = r#"await tools.exec_command({cmd:"cat \x61\u0062"});"#;
    let activity = analyze(source)?;
    assert_eq!(activity.disposition(), OrchestrationDisposition::Eligible);
    assert_eq!(activity.groups()[0].children()[0].command(), "cat ab");
    for source in [
        r#"await tools.exec_command({cmd:"cat \141"});"#,
        r#"await tools.exec_command({cmd:"cat \uD800"});"#,
        r#"await tools.exec_command({cmd:"cat \u{61}"});"#,
        r#"await tools.exec_command({cmd:"cat \q"});"#,
    ] {
        fallback(source)?;
    }
    Ok(())
}

#[test]
fn unknown_outer_or_child_runtime_never_produces_groups() -> TestResult {
    for (bindings, environment) in [
        (
            OrchestratorBindings::Unknown,
            EnvironmentAssumptions::FreshProcess,
        ),
        (
            OrchestratorBindings::TrustedFreshHostBindings,
            EnvironmentAssumptions::PersistentOrUnknown,
        ),
    ] {
        let input = CompleteOrchestration::from_source(
            "parent".into(),
            r#"await tools.exec_command({cmd:"cat a"});"#,
            bindings,
            environment,
            limits(),
        )?;
        let activity = Analyzer::new()?.analyze_orchestration(input);
        assert_eq!(
            activity.disposition(),
            OrchestrationDisposition::Fallback(OrchestrationFallback::EnvironmentUnknown)
        );
        assert!(activity.groups().is_empty());
    }
    Ok(())
}

#[test]
fn finite_limits_and_zero_deadline_are_atomic() -> TestResult {
    let children = std::iter::repeat_n(r#"tools.exec_command({cmd:"cat a"})"#, 9)
        .collect::<Vec<_>>()
        .join(",");
    fallback(&format!("await Promise.all([{children}]);"))?;
    let bindings = (0..9)
        .map(|i| format!("const a{i}=\"cat a\";"))
        .collect::<String>();
    fallback(&bindings)?;
    fallback(&format!("const a=\"{}\";", "x".repeat(16_321)))?;
    let input = CompleteOrchestration::from_source(
        "parent".into(),
        "await Promise.all([]);",
        OrchestratorBindings::TrustedFreshHostBindings,
        EnvironmentAssumptions::FreshProcess,
        AnalysisLimits::new(32 * 1024, Duration::ZERO),
    )?;
    let activity = Analyzer::new()?.analyze_orchestration(input);
    assert_eq!(
        activity.disposition(),
        OrchestrationDisposition::Fallback(OrchestrationFallback::TimeBudget)
    );
    assert!(activity.groups().is_empty());
    Ok(())
}

#[test]
fn source_overflow_poisoning_preserves_complete_input_typestate() -> TestResult {
    let mut buffer = OrchestrationBuffer::new(
        "parent".into(),
        OrchestratorBindings::Unknown,
        EnvironmentAssumptions::PersistentOrUnknown,
        AnalysisLimits::new(4, Duration::from_secs(1)),
    )?;
    buffer.append("abc")?;
    assert_eq!(
        buffer.append("de"),
        Err(OrchestrationInputError::SourceLimit)
    );
    assert_eq!(buffer.append(""), Err(OrchestrationInputError::Aborted));
    assert!(matches!(
        buffer.finish(),
        Err(OrchestrationInputError::Aborted)
    ));
    Ok(())
}
