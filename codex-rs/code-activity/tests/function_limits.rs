//! Limits are exercised through inert code strings, never executed programs.
use codex_code_activity::Analyzer;
use codex_code_activity::Effect;
use codex_code_activity::Language;
use codex_code_activity::MAX_OPERATIONS;
use codex_code_activity::Request;
use codex_code_activity::Target;
use pretty_assertions::assert_eq;
use std::fmt::Write;

#[test]
fn function_and_scope_growth_abstain_with_explicit_budgets() -> eyre::Result<()> {
    let mut definitions = String::new();
    for i in 0..200 {
        write!(&mut definitions, "const f{i}=()=>'{i}';")?;
    }
    for source in [
        definitions,
        format!("const f=p=>p;{}", "f('x');".repeat(300)),
    ] {
        let report = Analyzer::new()?.analyze(Request::new(
            "budget".into(),
            Language::TypeScript,
            source,
            None,
        ));
        assert!(report.operations.len() <= MAX_OPERATIONS);
        assert!(report.unresolved.len() <= MAX_OPERATIONS);
        assert!(
            report
                .unresolved
                .iter()
                .any(|gap| gap.reason.contains("budget")),
            "{report:?}"
        );
    }
    Ok(())
}

#[test]
fn a_recursive_result_cannot_restore_a_literal_target() -> eyre::Result<()> {
    let report = Analyzer::new()?.analyze(Request::new("recursive".into(), Language::TypeScript,
        "import fs from 'node:fs';function f(p:string){return f(p)}fs.unlinkSync(f('wrong'));import {writeFileSync} from 'node:fs';writeFileSync('independent','x');".into(), None));
    let targets: Vec<_> = report
        .operations
        .iter()
        .map(|op| match &op.effect {
            Effect::FileDelete { target } | Effect::FileWrite { target, .. } => target,
            other => panic!("unexpected operation: {other:?}"),
        })
        .collect();
    assert_eq!(
        targets,
        vec![
            &Target::Unresolved,
            &Target::Literal {
                path: "independent".into(),
                cwd: None
            }
        ]
    );
    assert!(
        report
            .unresolved
            .iter()
            .any(|gap| gap.reason.contains("Recursive"))
    );
    Ok(())
}

#[test]
fn called_body_spans_and_repeated_counts_are_preserved() -> eyre::Result<()> {
    let source = "const fs=require('fs');const f=p=>fs.unlinkSync(p);f('a');f('b');";
    let report = Analyzer::new()?.analyze(Request::new(
        "evidence".into(),
        Language::TypeScript,
        source.into(),
        None,
    ));
    assert_eq!(report.operations.len(), 2);
    for op in &report.operations {
        assert_eq!(op.evidence.source_id.index(), 0);
        assert_eq!(
            &source[op.evidence.start_byte..op.evidence.end_byte],
            "fs.unlinkSync(p)"
        );
    }
    assert_ne!(report.operations[0].id, report.operations[1].id);
    Ok(())
}
