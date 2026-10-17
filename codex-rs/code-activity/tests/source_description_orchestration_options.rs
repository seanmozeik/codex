//! Literal transport options describe source spelling without granting authority.

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
        "authored-transport-options".into(),
        Language::TypeScript,
        source,
        AnalysisLimits::default(),
    )?;
    Ok(Analyzer::new()?.describe_complete(input))
}

#[test]
fn literal_transport_fields_are_source_references_with_exact_pair_evidence()
-> Result<(), ActivityError> {
    let source = "const r=await tools.exec_command({cmd:'cat a',shell:'/bin/bash',sandbox_permissions:'require_escalated',justification:`Read the authored fixture`,prefix_rule:['cat',`a`,]}); text(r);";
    let description = describe(source)?;
    assert_eq!(
        description.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    let options: Vec<_> = description
        .references()
        .iter()
        .filter(|reference| reference.kind == SourceReferenceKind::CommandOption)
        .map(|reference| {
            assert_eq!(reference.basis, SourceReferenceBasis::SourceSyntax);
            assert_eq!(reference.evidence.source_id.index(), 0);
            (
                source.get(reference.evidence.start_byte..reference.evidence.end_byte),
                reference.literal.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        options,
        vec![
            (Some("shell:'/bin/bash'"), Some("/bin/bash")),
            (
                Some("sandbox_permissions:'require_escalated'"),
                Some("require_escalated")
            ),
            (
                Some("justification:`Read the authored fixture`"),
                Some("Read the authored fixture")
            ),
            (Some("prefix_rule:['cat',`a`,]"), None),
        ]
    );
    assert!(
        description
            .references()
            .iter()
            .all(|reference| reference.basis == SourceReferenceBasis::SourceSyntax)
    );
    Ok(())
}

#[test]
fn every_known_shell_and_permission_spelling_accepts_only_literal_source_options()
-> Result<(), ActivityError> {
    for shell in ["bash", "sh", "zsh", "/bin/bash", "/bin/sh", "/bin/zsh"] {
        for permission in ["use_default", "require_escalated"] {
            let source = format!(
                "await tools.exec_command({{cmd:'cat a',shell:'{shell}',sandbox_permissions:'{permission}',justification:'Read fixture',prefix_rule:[]}});"
            );
            assert_eq!(
                describe(&source)?.disposition(),
                SourceDescriptionDisposition::CompleteSyntax,
                "{source}"
            );
        }
    }
    Ok(())
}

#[test]
fn transport_options_keep_inline_python_child_descriptions_mandatory() -> Result<(), ActivityError>
{
    let source = r#"const r=await tools.exec_command({cmd:`python3 - <<'PY'
from pathlib import Path
Path('a').write_text('changed')
PY`,workdir:'/tmp/authored',shell:'/bin/sh',login:false,tty:false,sandbox_permissions:'use_default',justification:'Describe fixture',prefix_rule:['python3']}); text(r);"#;
    let description = describe(source)?;
    assert_eq!(
        description.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    assert_eq!(
        description
            .references()
            .iter()
            .filter(|reference| reference.kind == SourceReferenceKind::CommandOption)
            .count(),
        7
    );
    assert!(description.references().iter().any(|reference| {
        reference.kind == SourceReferenceKind::WriteCall
            && reference.evidence.source_id.index() != 0
    }));
    let unsupported = source.replace("Path('a').write_text('changed')", "mystery()");
    let description = describe(&unsupported)?;
    assert!(matches!(
        description.disposition(),
        SourceDescriptionDisposition::Fallback(_)
    ));
    assert!(description.references().is_empty());
    assert!(description.sources().is_empty());
    Ok(())
}

#[test]
fn malformed_dynamic_or_unknown_transport_options_discard_the_whole_envelope()
-> Result<(), ActivityError> {
    let prefix = "await tools.exec_command({cmd:'cat a'}); ";
    let suffixes = [
        "await tools.exec_command({cmd:'cat a',shell:'/bin/fish'});",
        "await tools.exec_command({cmd:'cat a',shell:'bash -c'});",
        "await tools.exec_command({cmd:'cat a',shell:'/usr/bin/bash'});",
        "const s='bash'; await tools.exec_command({cmd:'cat a',shell:s});",
        "await tools.exec_command({cmd:'cat a',shell:`${name}`});",
        "await tools.exec_command({cmd:'cat a',sandbox_permissions:'allow_all'});",
        "await tools.exec_command({cmd:'cat a',sandbox_permissions:true});",
        "const p='use_default'; await tools.exec_command({cmd:'cat a',sandbox_permissions:p});",
        "await tools.exec_command({cmd:'cat a',justification:reason()});",
        "await tools.exec_command({cmd:'cat a',justification:()=> 'reason'});",
        "await tools.exec_command({cmd:'cat a',justification:null});",
        "const why='Read'; await tools.exec_command({cmd:'cat a',justification:why});",
        "await tools.exec_command({cmd:'cat a',prefix_rule:rules});",
        "await tools.exec_command({cmd:'cat a',prefix_rule:['cat',name]});",
        "const name='cat'; await tools.exec_command({cmd:'cat a',prefix_rule:[name]});",
        "await tools.exec_command({cmd:'cat a',prefix_rule:['cat',...other]});",
        "await tools.exec_command({cmd:'cat a',prefix_rule:['cat',,]});",
        "await tools.exec_command({cmd:'cat a',prefix_rule:[['cat']]});",
        "await tools.exec_command({cmd:'cat a',prefix_rule:[1]});",
        "await tools.exec_command({cmd:'cat a',prefix_rule:['cat'].concat(['a'])});",
        "await tools.exec_command({cmd:'cat a',prefix_rule:[`cat ${name}`]});",
        "await tools.exec_command({cmd:'cat a',prefix_rule:['\\q']});",
        "await tools.exec_command({cmd:'cat a',['shell']:'bash'});",
        "await tools.exec_command({cmd:'cat a',shell:'bash',shell:'sh'});",
        "await tools.exec_command({cmd:'cat a',sandbox_permissions:'use_default','sandbox_permissions':'require_escalated'});",
        "await tools.exec_command({cmd:'cat a',prefix_rule:[],...options});",
        "await tools.exec_command({cmd:'cat a',shell:'bash',env:{}});",
        "await tools.exec_command({cmd:'cat a',shell:'bash'}); unknown();",
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
fn literal_array_and_object_limits_remain_eight() -> Result<(), ActivityError> {
    let eight =
        "await tools.exec_command({cmd:'cat a',prefix_rule:['0','1','2','3','4','5','6','7']});";
    assert_eq!(
        describe(eight)?.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    let over_limit = [
        "await tools.exec_command({cmd:'cat a',prefix_rule:['0','1','2','3','4','5','6','7','8']});",
        "await tools.exec_command({cmd:'cat a',workdir:'/tmp/authored',max_output_tokens:1,yield_time_ms:1,login:false,tty:false,shell:'sh',sandbox_permissions:'use_default',justification:'Read'});",
    ];
    for source in over_limit {
        let description = describe(source)?;
        assert_eq!(
            description.disposition(),
            SourceDescriptionDisposition::Fallback(SourceDescriptionFallback::SourceLimit),
            "{source}"
        );
        assert!(description.references().is_empty());
        assert!(description.sources().is_empty());
    }
    Ok(())
}
