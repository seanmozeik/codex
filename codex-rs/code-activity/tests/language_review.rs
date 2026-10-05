//! Independent language review: submitted Python/JS/TS programs remain inert data.
use codex_code_activity::Analyzer;
use codex_code_activity::Effect;
use codex_code_activity::Language;
use codex_code_activity::Report;
use codex_code_activity::Request;
use codex_code_activity::Target;
use codex_code_activity::WriteMode;
use pretty_assertions::assert_eq;

fn analyze(language: Language, source: &str) -> anyhow::Result<Report> {
    Ok(Analyzer::new()?.analyze(Request::new(
        "language-review".into(),
        language,
        source.into(),
        Some("/workspace".into()),
    )))
}

fn targets(report: &Report) -> Vec<&Target> {
    report
        .operations
        .iter()
        .filter_map(|operation| match &operation.effect {
            Effect::FileOpen { target, .. }
            | Effect::FileRead { target }
            | Effect::FileWrite { target, .. }
            | Effect::FileEdit { target, .. }
            | Effect::FileDelete { target }
            | Effect::FileTruncate { target }
            | Effect::FileList { target } => Some(target),
            _ => None,
        })
        .collect()
}

fn assert_callback_uncertainty(report: &Report, source: &str) {
    assert!(
        targets(report)
            .iter()
            .all(|target| !matches!(target, Target::Literal { path, .. } if path == "old")),
        "a callback can change the target; {source}: {report:?}"
    );
    assert!(
        report
            .unresolved
            .iter()
            .any(|gap| gap.reason.to_ascii_lowercase().contains("callback")),
        "callback uncertainty must be explicit; {source}: {report:?}"
    );
}

#[test]
fn javascript_callback_options_invalidate_enclosing_paths() -> anyhow::Result<()> {
    for expression in [
        "JSON.parse('{}',()=>{p='new';return 0})",
        "JSON.stringify({},()=>{p='new';return 0})",
        "'x'.replace(/x/,()=>{p='new';return 'x'})",
        "'x'.replaceAll('x',()=>{p='new';return 'x'})",
    ] {
        let source = format!("const fs=require('fs');let p='old';{expression};fs.unlinkSync(p)");
        let report = analyze(Language::TypeScript, &source)?;
        assert_callback_uncertainty(&report, &source);
    }
    Ok(())
}

#[test]
fn implicit_javascript_callbacks_inside_objects_invalidate_paths() -> anyhow::Result<()> {
    for expression in [
        "JSON.stringify({toJSON:()=>{p='new';return 0}})",
        "JSON.stringify([{toJSON:()=>{p='new';return 0}}])",
        "'x'.replace('x',{toString:()=>{p='new';return 'x'}})",
    ] {
        let source = format!("const fs=require('fs');let p='old';{expression};fs.unlinkSync(p)");
        let report = analyze(Language::TypeScript, &source)?;
        assert_callback_uncertainty(&report, &source);
    }
    Ok(())
}

#[test]
fn unknown_javascript_callback_values_invalidate_paths() -> anyhow::Result<()> {
    for expression in [
        "JSON.parse('{}',callback)",
        "JSON.stringify({},callback)",
        "'x'.replace('x',callback)",
    ] {
        let source = format!("const fs=require('fs');let p='old';{expression};fs.unlinkSync(p)");
        let report = analyze(Language::TypeScript, &source)?;
        assert_callback_uncertainty(&report, &source);
    }
    Ok(())
}

#[test]
fn python_callback_keywords_do_not_preserve_old_paths() -> anyhow::Result<()> {
    for expression in [
        "sorted([1],key=lambda x: mutate())",
        "min([1],key=lambda x: mutate())",
        "max([1],key=lambda x: mutate())",
        "sorted([1],key=callback)",
        "json.loads('{}',object_hook=callback)",
        "json.dumps({},default=callback)",
    ] {
        let source = format!("import os,json\np='old'\n{expression}\nos.remove(p)");
        let report = analyze(Language::Python, &source)?;
        assert_callback_uncertainty(&report, &source);
    }
    Ok(())
}

#[test]
fn inert_json_options_keep_independent_file_targets() -> anyhow::Result<()> {
    for (language, source) in [
        (
            Language::TypeScript,
            "const fs=require('fs');JSON.stringify({ x:1 },null,2);fs.unlinkSync('after')",
        ),
        (
            Language::Python,
            "import os,json\njson.dumps({'x':1},indent=2)\nos.remove('after')",
        ),
    ] {
        let report = analyze(language, source)?;
        assert_eq!(
            targets(&report),
            vec![&Target::Literal {
                path: "after".into(),
                cwd: Some("/workspace".into()),
            }],
            "{source}: {:?}",
            report.unresolved
        );
    }
    Ok(())
}

#[test]
fn literal_callbacks_have_explicit_iteration_limits() -> anyhow::Result<()> {
    for length in [128, 129] {
        let items = vec!["'a'"; length].join(",");
        let source = format!("const fs=require('fs');[{items}].forEach(p=>fs.unlinkSync(p))");
        let report = analyze(Language::TypeScript, &source)?;
        if length == 128 {
            assert_eq!(report.operations.len(), length);
            assert!(report.unresolved.is_empty(), "{report:?}");
        } else {
            assert!(report.operations.is_empty(), "{report:?}");
            assert!(
                report
                    .unresolved
                    .iter()
                    .any(|gap| gap.reason.contains("bounds")),
                "{report:?}"
            );
        }
    }
    Ok(())
}

#[test]
fn typed_wrapper_preserves_nested_tool_source_and_workdir() -> anyhow::Result<()> {
    let source = r#"const run=(cmd:string)=>tools.exec_command({cmd,workdir:'/nested'});run("python3 -c \"open('a','w')\"")"#;
    let report = analyze(Language::TypeScript, source)?;
    assert_eq!(
        report
            .operations
            .iter()
            .map(|op| &op.effect)
            .collect::<Vec<_>>(),
        vec![&Effect::FileOpen {
            target: Target::Literal {
                path: "a".into(),
                cwd: Some("/nested".into()),
            },
            mode: WriteMode::Replace,
        }]
    );
    assert_eq!(report.sources.len(), 3);
    assert!(
        report
            .sources
            .iter()
            .skip(1)
            .all(|source| source.parent.is_some())
    );
    Ok(())
}

#[test]
fn uncalled_unsupported_bodies_do_not_emit_file_activity() -> anyhow::Result<()> {
    for (language, source) in [
        (
            Language::TypeScript,
            "const fs=require('fs');function f(p=fs.unlinkSync('default')){fs.unlinkSync('body')}",
        ),
        (
            Language::TypeScript,
            "function f(){try{require('fs').unlinkSync('a')}finally{require('fs').unlinkSync('b')}}",
        ),
        (
            Language::Python,
            "def f():\n try:\n  open('a','w')\n finally:\n  open('b','w')",
        ),
    ] {
        let report = analyze(language, source)?;
        assert!(report.operations.is_empty(), "{source}: {report:?}");
    }
    Ok(())
}

#[test]
fn python_defaults_execute_at_definition_without_executing_body() -> anyhow::Result<()> {
    let report = analyze(
        Language::Python,
        "def f(p=open('default','w')):\n open('body','w')",
    )?;
    assert_eq!(
        targets(&report),
        vec![&Target::Literal {
            path: "default".into(),
            cwd: Some("/workspace".into()),
        }]
    );
    Ok(())
}

#[test]
fn tripwire_outputs_do_not_override_language_semantics() -> anyhow::Result<()> {
    // Actual Tripwire 56f18dad produces literal activity for these cases.
    // Its policy analyzer is useful evidence, but is not a sound activity oracle.
    for (language, source) in [
        (
            Language::TypeScript,
            "const fs=require('fs');let p='old';function f(){{var p='new'}return p}fs.unlinkSync(f())",
        ),
        (
            Language::TypeScript,
            "const fs=require('fs');const f=async()=> 'old';fs.unlinkSync(f())",
        ),
        (Language::Python, "def f(p):\n open('old','w')\nf()"),
        (
            Language::Python,
            "def f():\n open('old','w')\n import json as open\nf()",
        ),
    ] {
        let report = analyze(language, source)?;
        assert!(
            targets(&report).iter().all(|target| !matches!(target,
                Target::Literal { path, .. } if path == "old")),
            "{source}: {report:?}"
        );
        assert!(!report.unresolved.is_empty(), "{source}: {report:?}");
    }
    Ok(())
}
