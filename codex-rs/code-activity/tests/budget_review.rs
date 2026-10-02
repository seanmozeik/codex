//! Combined adversarial sources remain inert and respect report-wide limits.
use codex_code_activity::Analyzer;
use codex_code_activity::Language;
use codex_code_activity::MAX_OPERATIONS;
use codex_code_activity::Request;
use pretty_assertions::assert_eq;

#[test]
fn malformed_children_cannot_overflow_a_saturated_report() -> eyre::Result<()> {
    let source = "printf x;".repeat(300) + &"python3 -c 'if';".repeat(65);
    let report = Analyzer::new()?.analyze(Request::new(
        "combined-bounds".into(),
        Language::Shell,
        source,
        None,
    ));
    assert!(report.operations.len() <= MAX_OPERATIONS);
    assert!(report.unresolved.len() <= MAX_OPERATIONS);
    assert!(report.sources.len() <= 64);
    assert!(!report.unresolved.is_empty());
    assert!(
        report
            .operations
            .iter()
            .all(|operation| operation.evidence.source_id.index() < report.sources.len())
    );
    assert!(
        report
            .unresolved
            .iter()
            .all(|gap| gap.evidence.source_id.index() < report.sources.len())
    );
    Ok(())
}

#[test]
fn nested_tool_execution_context_is_never_silently_assumed() -> eyre::Result<()> {
    for options in [
        "shell:'/bin/pwsh'",
        "shell:selectedShell",
        "login:true",
        "login:selectedLogin",
    ] {
        let source = format!("tools.exec_command({{cmd:'cat a',{options}}})");
        let report = Analyzer::new()?.analyze(Request::new(
            "tool-context".into(),
            Language::TypeScript,
            source,
            None,
        ));
        assert!(report.operations.is_empty(), "{report:?}");
        assert!(
            report
                .unresolved
                .iter()
                .any(|gap| gap.reason.contains("context options"))
        );
    }
    let source = "tools.exec_command({cmd:'cat a',shell:'/bin/bash',login:false})";
    let report = Analyzer::new()?.analyze(Request::new(
        "tool-context".into(),
        Language::TypeScript,
        source.into(),
        None,
    ));
    assert_eq!(report.operations.len(), 1);
    Ok(())
}

#[test]
fn typescript_type_only_imports_supply_no_runtime_api() -> eyre::Result<()> {
    for source in [
        "import type fs from 'node:fs';fs.unlinkSync('a')",
        "import type * as fs from 'node:fs';fs.unlinkSync('a')",
        "import {type unlinkSync as remove} from 'node:fs';remove('a')",
    ] {
        let report = Analyzer::new()?.analyze(Request::new(
            "type-only".into(),
            Language::TypeScript,
            source.into(),
            None,
        ));
        assert!(report.operations.is_empty(), "{source}: {report:?}");
        assert!(
            report
                .unresolved
                .iter()
                .any(|gap| gap.reason.contains("Type-only"))
        );
    }
    let source = "import {type Stats, unlinkSync} from 'node:fs';unlinkSync('a')";
    let report = Analyzer::new()?.analyze(Request::new(
        "mixed-import".into(),
        Language::TypeScript,
        source.into(),
        None,
    ));
    assert_eq!(report.operations.len(), 1);
    Ok(())
}
