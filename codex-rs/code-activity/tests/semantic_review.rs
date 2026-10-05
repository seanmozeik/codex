//! Inert regression programs for lazy values, asynchronous APIs and shared budgets.
use codex_code_activity::Analyzer;
use codex_code_activity::Effect;
use codex_code_activity::Language;
use codex_code_activity::Report;
use codex_code_activity::Request;
use codex_code_activity::Target;
use pretty_assertions::assert_eq;
use std::fmt::Write;

fn analyze(language: Language, source: &str) -> anyhow::Result<Report> {
    Ok(Analyzer::new()?.analyze(Request::new(
        "semantic-review".into(),
        language,
        source.into(),
        /*cwd*/ None,
    )))
}

#[test]
fn unused_generator_never_reports_lazy_body_effects() -> anyhow::Result<()> {
    let report = analyze(Language::Python, "g = (open('never','w') for p in ['a'])")?;
    assert!(report.operations.is_empty());
    assert!(
        report
            .unresolved
            .iter()
            .any(|gap| gap.reason.contains("Lazy generator"))
    );
    Ok(())
}

#[test]
fn generator_creation_retains_only_outer_iterable_effects() -> anyhow::Result<()> {
    let report = analyze(
        Language::Python,
        "g = (open('never','w') for p in open('outer').read())",
    )?;
    assert_eq!(report.operations.len(), 1);
    assert!(
        matches!(&report.operations[0].effect, Effect::FileRead { target: Target::Literal { path, .. } } if path == "outer")
    );
    Ok(())
}

#[test]
fn consuming_an_unsupported_generator_abstains_instead_of_eagerly_running_it() -> anyhow::Result<()>
{
    let report = analyze(Language::Python, "list(open('never','w') for p in ['a'])")?;
    assert!(report.operations.is_empty());
    assert!(!report.unresolved.is_empty());
    Ok(())
}

#[test]
fn dynamic_import_requires_await_before_module_api_access() -> anyhow::Result<()> {
    let deferred = analyze(
        Language::TypeScript,
        "const fs=import('node:fs');fs.unlinkSync('never')",
    )?;
    let resolved = analyze(
        Language::TypeScript,
        "const fs=await import('node:fs');fs.unlinkSync('target')",
    )?;
    assert!(deferred.operations.is_empty());
    assert!(!deferred.unresolved.is_empty());
    assert!(
        matches!(&resolved.operations[0].effect, Effect::FileDelete { target: Target::Literal { path, .. } } if path == "target")
    );
    Ok(())
}

#[test]
fn promise_file_content_requires_await_before_text_edit_inference() -> anyhow::Result<()> {
    for import in [
        "const fs=require('node:fs/promises');",
        "const fs=require('node:fs').promises;",
    ] {
        let deferred = analyze(
            Language::TypeScript,
            &format!(
                "{import}const s=fs.readFile('a','utf8');fs.writeFile('a',s.replace('old','new'))"
            ),
        )?;
        let resolved = analyze(
            Language::TypeScript,
            &format!(
                "{import}const s=await fs.readFile('a','utf8');await fs.writeFile('a',s.replace('old','new'))"
            ),
        )?;
        assert!(
            !deferred
                .operations
                .iter()
                .any(|op| matches!(op.effect, Effect::FileEdit { .. }))
        );
        assert!(!deferred.unresolved.is_empty());
        assert!(
            resolved
                .operations
                .iter()
                .any(|op| matches!(op.effect, Effect::FileEdit { .. }))
        );
    }
    Ok(())
}

#[test]
fn callback_file_reads_and_binary_buffers_do_not_supply_text_edit_content() -> anyhow::Result<()> {
    for source in [
        "const fs=require('node:fs');const s=fs.readFile('a','utf8');fs.writeFileSync('a',s.replace('x','y'))",
        "const fs=require('node:fs');const s=fs.readFileSync('a');fs.writeFileSync('a',s.replace('x','y'))",
        "const fs=require('node:fs/promises');const s=await fs.readFile('a');await fs.writeFile('a',s.replace('x','y'))",
    ] {
        let report = analyze(Language::TypeScript, source)?;
        assert!(
            !report
                .operations
                .iter()
                .any(|op| matches!(op.effect, Effect::FileEdit { .. })),
            "{source}"
        );
    }
    Ok(())
}

#[test]
fn promise_namespace_has_no_sync_methods() -> anyhow::Result<()> {
    let report = analyze(
        Language::TypeScript,
        "const fs=require('node:fs/promises');fs.unlinkSync('never')",
    )?;
    assert!(report.operations.is_empty());
    assert!(!report.unresolved.is_empty());
    Ok(())
}

#[test]
fn import_binding_budget_is_enforced_before_out_of_budget_alias_use() -> anyhow::Result<()> {
    let aliases = (0..300)
        .map(|i| format!("remove as name{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let report = analyze(
        Language::Python,
        &format!("from os import {aliases}\nname299('never')"),
    )?;
    assert!(report.operations.is_empty());
    assert!(
        report
            .unresolved
            .iter()
            .any(|gap| gap.reason.contains("binding budget"))
    );
    Ok(())
}

#[test]
fn preseed_binding_exhaustion_never_exposes_an_enclosing_builtin() -> anyhow::Result<()> {
    let mut declarations = String::new();
    for index in 0..300 {
        writeln!(&mut declarations, "    v{index}=None")?;
    }
    let report = analyze(
        Language::Python,
        &format!("def f():\n{declarations}    open('never','w')\n    open=None\nf()"),
    )?;
    assert!(report.operations.is_empty());
    assert!(
        report
            .unresolved
            .iter()
            .any(|gap| gap.reason.contains("binding budget"))
    );
    Ok(())
}
