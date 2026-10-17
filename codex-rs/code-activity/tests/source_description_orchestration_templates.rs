//! Literal template cooking and boolean source options never confer runtime trust.

use codex_code_activity::ActivityError;
use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::CompleteSourceDescriptionInput;
use codex_code_activity::Language;
use codex_code_activity::SourceDescription;
use codex_code_activity::SourceDescriptionDisposition;
use codex_code_activity::SourceReferenceKind;
use pretty_assertions::assert_eq;

fn describe(source: &str) -> Result<SourceDescription, ActivityError> {
    let input = CompleteSourceDescriptionInput::from_source(
        "authored-literal-template".into(),
        Language::TypeScript,
        source,
        AnalysisLimits::default(),
    )?;
    Ok(Analyzer::new()?.describe_complete(input))
}

#[test]
fn quoted_and_multiline_template_heredocs_describe_the_same_single_invocation()
-> Result<(), ActivityError> {
    let quoted = r#"const r=await tools.exec_command({cmd:"python3 - <<'PY'\nfrom pathlib import Path\nPath('a').write_text('changed')\nPY",login:false,tty:true}); text(r);"#;
    let template = r#"const r=await tools.exec_command({cmd:`python3 - <<'PY'
from pathlib import Path
Path('a').write_text('changed')
PY`,login:false,tty:true}); text(r);"#;
    let quoted = describe(quoted)?;
    let template = describe(template)?;
    assert_eq!(
        quoted.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    assert_eq!(
        template.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    let references = |value: &SourceDescription| {
        value
            .references()
            .iter()
            .map(|reference| (reference.kind, reference.literal.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(references(&quoted), references(&template));
    assert_eq!(
        template
            .references()
            .iter()
            .filter(
                |reference| reference.kind == SourceReferenceKind::InvocationCall
                    && reference.evidence.source_id.index() == 0
            )
            .count(),
        1
    );
    assert!(
        template
            .references()
            .iter()
            .any(|reference| reference.kind == SourceReferenceKind::WriteCall)
    );
    Ok(())
}

#[test]
fn literal_templates_work_in_direct_calls_bindings_and_exact_display_wrappers()
-> Result<(), ActivityError> {
    let sources = [
        "text(await tools.exec_command({cmd:`cat a`,login:false,tty:false}));",
        "const cmd=`cat a`; const r=await tools.exec_command({cmd:cmd,login:true,tty:true}); text(r);",
        "const rs=await Promise.all([tools.exec_command({cmd:`cat a`}),tools.exec_command({cmd:`cat b`})]); rs.forEach(text);",
        "await tools.exec_command({cmd:`cat 'a\\`b' '\\${name}'`});",
        "await tools.exec_command({cmd:`cat a\\\n b`});",
        "await tools.exec_command({cmd:`cat a\\\r\n b`});",
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
fn login_and_tty_are_literal_source_options_only() -> Result<(), ActivityError> {
    let source = "await tools.exec_command({cmd:`cat a`,login:false,'tty':true});";
    let description = describe(source)?;
    assert_eq!(
        description.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    let options: Vec<_> = description
        .references()
        .iter()
        .filter(|reference| reference.kind == SourceReferenceKind::CommandOption)
        .map(|reference| reference.literal.as_deref())
        .collect();
    assert_eq!(options, vec![Some("false"), Some("true")]);
    Ok(())
}

#[test]
fn dynamic_invalid_and_tagged_templates_clear_all_outer_and_child_references()
-> Result<(), ActivityError> {
    let prefix = "await tools.exec_command({cmd:`cat a`}); ";
    let suffixes = [
        "await tools.exec_command({cmd:`cat ${name}`});",
        "const name='a'; await tools.exec_command({cmd:`cat ${name}`});",
        "await tools.exec_command({cmd:String.raw`cat a`});",
        "await tools.exec_command({cmd:tag`cat a`});",
        "await tools.exec_command({cmd:`cat \\xZZ`});",
        "await tools.exec_command({cmd:`cat \\8`});",
        "await tools.exec_command({cmd:`cat \\q`});",
        "await tools.exec_command({cmd:`cat a\r\nb`});",
        "const cmd=`cat a`; cmd=`cat b`; await tools.exec_command({cmd:cmd});",
        "await tools.exec_command({cmd:`cat a`,...opts});",
        "await tools.exec_command({cmd:`cat a`,login:'false'});",
        "await tools.exec_command({cmd:`cat a`,login:!false});",
        "await tools.exec_command({cmd:`cat a`,tty:1});",
        "await tools.exec_command({cmd:`cat a`,tty:false,tty:true});",
        "await tools.exec_command({cmd:`cat a`,shell:'/bin/fish'});",
        "await tools.exec_command({cmd:`cat a`,sandbox_permissions:'allow_all'});",
        "await tools.exec_command({cmd:`python3 -c 'mystery()'`});",
        "await tools.exec_command({cmd:`cat a`}); unknown();",
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
