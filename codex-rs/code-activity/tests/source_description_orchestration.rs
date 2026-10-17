//! Complete orchestration source descriptions and atomic fallback boundaries.

use codex_code_activity::ActivityError;
use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::CompleteOrchestration;
use codex_code_activity::CompleteSourceDescriptionInput;
use codex_code_activity::EnvironmentAssumptions;
use codex_code_activity::Language;
use codex_code_activity::OrchestrationDisposition;
use codex_code_activity::OrchestratorBindings;
use codex_code_activity::SourceDescription;
use codex_code_activity::SourceDescriptionDisposition;
use codex_code_activity::SourceDescriptionFallback;
use codex_code_activity::SourceReferenceBasis;
use codex_code_activity::SourceReferenceKind;
use pretty_assertions::assert_eq;
use std::time::Duration;

fn describe(source: &str) -> Result<SourceDescription, ActivityError> {
    let input = CompleteSourceDescriptionInput::from_source(
        "authored-wrapper".into(),
        Language::TypeScript,
        source,
        AnalysisLimits::default(),
    )?;
    Ok(Analyzer::new()?.describe_complete(input))
}

#[test]
fn literal_options_and_result_display_have_source_only_references() -> Result<(), ActivityError> {
    let source = "const r = await tools.exec_command({cmd:'cat a',workdir:'/tmp/authored',max_output_tokens:2000,yield_time_ms:1000}); text(r);";
    let description = describe(source)?;
    assert_eq!(
        description.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    let outer: Vec<_> = description
        .references()
        .iter()
        .filter(|reference| reference.evidence.source_id.index() == 0)
        .map(|reference| (reference.kind, reference.literal.as_deref()))
        .collect();
    assert_eq!(
        outer,
        vec![
            (SourceReferenceKind::InvocationCall, None),
            (SourceReferenceKind::CommandOption, Some("/tmp/authored")),
            (SourceReferenceKind::CommandOption, Some("2000")),
            (SourceReferenceKind::CommandOption, Some("1000")),
            (SourceReferenceKind::OutputConsumer, None),
        ]
    );
    for reference in description.references() {
        assert_eq!(reference.basis, SourceReferenceBasis::SourceSyntax);
        if reference.evidence.source_id.index() == 0 {
            assert!(
                source
                    .get(reference.evidence.start_byte..reference.evidence.end_byte)
                    .is_some()
            );
        }
    }
    Ok(())
}

#[test]
fn direct_and_joined_output_wrappers_are_described_without_unrolling() -> Result<(), ActivityError>
{
    let sources = [
        "const r=await tools.exec_command({cmd:'cat a'}); text(r);",
        "text(await tools.exec_command({cmd:'cat a'}));",
        "const rs=await Promise.all([tools.exec_command({cmd:'cat a'})]); text(rs);",
        "const rs=await Promise.allSettled([tools.exec_command({cmd:'cat a'})]); rs.forEach(text);",
        "const rs=await Promise.all([tools.exec_command({cmd:'cat a'})]); rs.forEach(r=>text(r));",
        "const rs=await Promise.all([tools.exec_command({cmd:'cat a'})]); rs.forEach((r)=>text(r));",
        "const rs=await Promise.all([tools.exec_command({cmd:'cat a'})]); for(const r of rs) text(r);",
        "const rs=await Promise.all([tools.exec_command({cmd:'cat a'})]); for(const r of rs) {text(r);}",
    ];
    for source in sources {
        let description = describe(source)?;
        assert_eq!(
            description.disposition(),
            SourceDescriptionDisposition::CompleteSyntax,
            "{source}"
        );
        let kinds: Vec<_> = description
            .references()
            .iter()
            .filter(|reference| reference.evidence.source_id.index() == 0)
            .map(|reference| reference.kind)
            .collect();
        assert_eq!(
            kinds,
            vec![
                SourceReferenceKind::InvocationCall,
                SourceReferenceKind::OutputConsumer
            ],
            "{source}"
        );
    }
    Ok(())
}

#[test]
fn literal_aliases_comments_asi_and_trailing_commas_are_accounted_for() -> Result<(), ActivityError>
{
    let sources = [
        "const command='cat a'; const copy=command; await tools.exec_command({cmd:copy});",
        "const dir='/tmp/authored'; await tools.exec_command({'cmd':'cat a',workdir:dir,yield_time_ms:0});",
        "const r=await tools.exec_command({cmd:'cat a'})\ntext(r)\n",
        "const r=await tools.exec_command({cmd:'cat a',},); text(r);",
        "await Promise.all([tools.exec_command({cmd:'cat a'}),]);",
        "/*outer*/ const r=await tools /*object*/ . exec_command({/*key*/cmd:'cat a'}); /*output*/text(r);",
    ];
    for source in sources {
        assert_eq!(
            describe(source)?.disposition(),
            SourceDescriptionDisposition::CompleteSyntax,
            "{source}"
        );
    }
    Ok(())
}

#[test]
fn closed_projection_and_callback_promotions_preserve_complete_sources() -> Result<(), ActivityError>
{
    for (prefix, suffix) in [
        (
            "await tools.exec_command({cmd:'cat a'}); ",
            "const r=await tools.exec_command({cmd:'cat b'}); text(r.output);",
        ),
        (
            "const rs=await Promise.all([tools.exec_command({cmd:'cat a'})]); ",
            "rs.forEach((r,i)=>text(r));",
        ),
        (
            "const rs=await Promise.all([tools.exec_command({cmd:'cat a'})]); ",
            "rs.forEach(r=>{text(r);});",
        ),
        (
            "const rs=await Promise.all([tools.exec_command({cmd:'cat a'})]); ",
            "rs.forEach(r=>text(rs));",
        ),
    ] {
        let source = format!("{prefix}{suffix}");
        let description = describe(&source)?;
        assert_eq!(
            description.disposition(),
            SourceDescriptionDisposition::CompleteSyntax,
            "{source}"
        );
        assert!(!description.sources().is_empty(), "{source}");
        assert!(!description.references().is_empty(), "{source}");
        assert_eq!(description.source_texts().first(), Some(&source));
        assert!(
            description
                .references()
                .iter()
                .all(|reference| reference.basis == SourceReferenceBasis::SourceSyntax),
            "{source}"
        );
    }
    Ok(())
}

#[test]
fn every_unsupported_wrapper_discards_the_recognized_prefix() -> Result<(), ActivityError> {
    let prefix = "await tools.exec_command({cmd:'cat a'}); ";
    let suffixes = [
        "unknown();",
        "text(unknown);",
        "const text='shadow';",
        "const tools='shadow';",
        "const Promise='shadow';",
        "await tools.exec_command({cmd:'cat a',timeout_ms:1000});",
        "await tools.exec_command({cmd:'cat a',shell:'/bin/fish'});",
        "await tools.exec_command({cmd:'cat a','cmd':'cat b'});",
        "await tools.exec_command({['cmd']:'cat a'});",
        "const cmd='cat b'; await tools.exec_command({cmd});",
        "await tools.exec_command({cmd:'cat a',...options});",
        "await tools.exec_command({get cmd(){return 'cat a'}});",
        "await tools.exec_command({cmd:'cat a',max_output_tokens:1+1});",
        "await tools.exec_command({cmd:'cat a',yield_time_ms:-1});",
        "await tools.exec_command({cmd:'cat a',yield_time_ms:1e3});",
        "await tools.exec_command({cmd:'cat a',yield_time_ms:9007199254740992});",
        "await tools.exec_command({cmd:'cat a',yield_time_ms:01});",
        "await tools.exec_command({cmd:`cat ${name}`});",
        "await tools?.exec_command({cmd:'cat a'});",
        "await tools.exec_command<string>({cmd:'cat a'});",
        "const r=await tools.exec_command({cmd:'cat b'}); await tools.exec_command({cmd:r});",
        "const cmd='cat b'; cmd='cat c';",
        "await Promise.all([tools.exec_command({cmd:'cat b'}),unknown()]);",
        "await Promise.all([tools.exec_command({cmd:'cat b'}),,]);",
        "await Promise.all([await tools.exec_command({cmd:'cat b'})]);",
        "await tools.exec_command({cmd:'cat b'}",
    ];
    for suffix in suffixes {
        let description = describe(&format!("{prefix}{suffix}"))?;
        assert!(
            matches!(
                description.disposition(),
                SourceDescriptionDisposition::Fallback(_)
            ),
            "{suffix}"
        );
        assert!(description.references().is_empty(), "{suffix}");
        assert!(description.sources().is_empty(), "{suffix}");
    }
    Ok(())
}

#[test]
fn display_loops_accept_no_extra_behavior_or_shadowing() -> Result<(), ActivityError> {
    let prefix = "const rs=await Promise.all([tools.exec_command({cmd:'cat a'})]); ";
    let suffixes = [
        "for(const r in rs) text(r);",
        "for(let r of rs) text(r);",
        "for await(const r of rs) text(r);",
        "for(const r of rs) {text(r); unknown();}",
        "for(const r of rs) {await tools.exec_command({cmd:'cat b'});}",
        "for(const text of rs) text(text);",
        "for(const rs of rs) text(rs);",
        "for(const r of unknown) text(r);",
        "rs.forEach((r='x')=>text(r));",
        "rs.forEach((r:string)=>text(r));",
        "rs.forEach(async r=>text(r));",
        "rs.forEach(r=>unknown(r));",
        "rs.forEach(text, unknown);",
        "rs.forEach((...r)=>text(r));",
        "rs.forEach(r=>text(r)); text(r);",
        "rs.map(text);",
    ];
    for suffix in suffixes {
        let description = describe(&format!("{prefix}{suffix}"))?;
        assert!(
            matches!(
                description.disposition(),
                SourceDescriptionDisposition::Fallback(_)
            ),
            "{suffix}"
        );
        assert!(description.references().is_empty(), "{suffix}");
    }
    Ok(())
}

#[test]
fn recognized_child_failures_clear_outer_options_and_consumers() -> Result<(), ActivityError> {
    let sources = [
        "const r=await tools.exec_command({cmd:'python3 -c \"mystery()\"',workdir:'/tmp/authored'}); text(r);",
        "await tools.exec_command({cmd:'python3 -X custom -c \"print(1)\"'});",
        "await tools.exec_command({cmd:'python3 authored.py'});",
        "await tools.exec_command({cmd:'cat $(pwd)'});",
    ];
    for source in sources {
        let description = describe(source)?;
        assert!(
            matches!(
                description.disposition(),
                SourceDescriptionDisposition::Fallback(_)
            ),
            "{source}"
        );
        assert!(description.references().is_empty());
        assert!(description.sources().is_empty());
    }
    Ok(())
}

#[test]
fn invocation_and_binding_budgets_never_keep_a_prefix() -> Result<(), ActivityError> {
    let sources = [
        "await tools.exec_command({cmd:'cat a'});".repeat(9),
        (0..9)
            .map(|i| format!("const value{i}='x';"))
            .collect::<String>(),
        format!(
            "await tools.exec_command({{cmd:'{}'}});",
            "x".repeat(16_321)
        ),
    ];
    for source in sources {
        let description = describe(&source)?;
        assert_eq!(
            description.disposition(),
            SourceDescriptionDisposition::Fallback(SourceDescriptionFallback::SourceLimit)
        );
        assert!(description.references().is_empty());
    }
    Ok(())
}

#[test]
fn a_zero_description_deadline_returns_no_outer_or_child_rows() -> Result<(), ActivityError> {
    let input = CompleteSourceDescriptionInput::from_source(
        "expired-wrapper".into(),
        Language::TypeScript,
        "await tools.exec_command({cmd:'cat a'});",
        AnalysisLimits::new(32 * 1024, Duration::ZERO),
    )?;
    let description = Analyzer::new()?.describe_complete(input);
    assert_eq!(
        description.disposition(),
        SourceDescriptionDisposition::Fallback(SourceDescriptionFallback::TimeBudget)
    );
    assert!(description.references().is_empty());
    Ok(())
}

#[test]
fn source_wrapper_support_does_not_expand_the_trusted_effect_api()
-> Result<(), Box<dyn std::error::Error>> {
    let source = "const r=await tools.exec_command({cmd:'cat a',yield_time_ms:1000}); text(r);";
    let mut analyzer = Analyzer::new()?;
    let input = CompleteOrchestration::from_source(
        "legacy-wrapper".into(),
        source,
        OrchestratorBindings::TrustedFreshHostBindings,
        EnvironmentAssumptions::FreshProcess,
        AnalysisLimits::default(),
    )?;
    let activity = analyzer.analyze_orchestration(input);
    assert!(matches!(
        activity.disposition(),
        OrchestrationDisposition::Fallback(_)
    ));
    assert!(activity.groups().is_empty());
    assert_eq!(
        describe(source)?.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    Ok(())
}
