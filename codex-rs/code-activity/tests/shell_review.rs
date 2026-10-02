//! Independent shell regressions. Submitted programs remain inert strings.
use codex_code_activity::Analyzer;
use codex_code_activity::Effect;
use codex_code_activity::Language;
use codex_code_activity::Report;
use codex_code_activity::Request;
use codex_code_activity::Target;
use pretty_assertions::assert_eq;

fn analyze(source: &str) -> eyre::Result<Report> {
    Ok(Analyzer::new()?.analyze(Request::new(
        "shell-review".into(),
        Language::Shell,
        source.into(),
        Some("/workspace".into()),
    )))
}

fn reads(report: &Report) -> Vec<(&str, Option<&str>)> {
    report
        .operations
        .iter()
        .filter_map(|operation| match &operation.effect {
            Effect::FileRead {
                target: Target::Literal { path, cwd },
            } => Some((path.as_str(), cwd.as_deref())),
            _ => None,
        })
        .collect()
}

fn gap(report: &Report, reason: &str) -> bool {
    report
        .unresolved
        .iter()
        .any(|item| item.reason.contains(reason))
}

#[test]
fn directory_flags_never_become_path_components() -> eyre::Result<()> {
    let report = analyze("cd -- /tmp; cat a")?;
    assert_eq!(reads(&report), vec![("a", Some("/tmp"))]);
    for source in ["cd -P /tmp; cat a", "cd -; cat a", "cd /a /b; cat a"] {
        let report = analyze(source)?;
        assert_eq!(reads(&report), vec![("a", None)], "{source}");
        assert!(gap(&report, "Directory options"));
    }
    Ok(())
}

#[test]
fn pipeline_stages_do_not_change_sibling_or_caller_directories() -> eyre::Result<()> {
    let report = analyze("cd /elsewhere | cat a; cat b")?;
    assert_eq!(
        reads(&report),
        vec![("a", Some("/workspace")), ("b", Some("/workspace"))]
    );
    assert!(gap(&report, "pipeline"));
    let report = analyze("cat a | cd /other; cat b")?;
    assert_eq!(reads(&report), vec![("a", Some("/workspace")), ("b", None)]);
    let report = analyze("cat a | source environment.sh; python3 -c 'open(\"b\",\"w\")'")?;
    assert_eq!(report.sources.len(), 1);
    assert!(gap(&report, "Executable identity"));
    Ok(())
}

#[test]
fn conditional_directory_changes_join_to_unknown() -> eyre::Result<()> {
    for source in [
        "cd /first || cd /second; cat a",
        "cd /first && cat a; cat b",
    ] {
        let report = analyze(source)?;
        assert!(
            reads(&report).iter().all(|(_, cwd)| cwd.is_none()),
            "{report:?}"
        );
        assert!(gap(&report, "conditional") || gap(&report, "condition"));
    }
    let report = analyze("printf x && cat a")?;
    assert_eq!(reads(&report), vec![("a", Some("/workspace"))]);
    Ok(())
}

#[test]
fn competing_input_redirects_never_supply_interpreter_source() -> eyre::Result<()> {
    for source in [
        "python3 <<'PY' < script.py\nopen('a','w')\nPY",
        "python3 < script.py <<'PY'\nopen('a','w')\nPY",
        "python3 2<<'PY'\nopen('a','w')\nPY",
    ] {
        let report = analyze(source)?;
        assert_eq!(report.sources.len(), 1, "{source}: {report:?}");
        assert!(
            report
                .operations
                .iter()
                .all(|operation| matches!(operation.effect, Effect::ProcessRun { .. })),
            "{report:?}"
        );
        assert!(gap(&report, "input"), "{report:?}");
    }
    let report = analyze("python3 <<'PY'\nopen('a','w')\nPY")?;
    assert_eq!(report.sources.len(), 2);
    assert!(
        report
            .operations
            .iter()
            .any(|operation| matches!(operation.effect, Effect::FileOpen { .. }))
    );
    Ok(())
}

#[test]
fn stdout_redirects_remain_uncertain_alongside_literal_input() -> eyre::Result<()> {
    for source in [
        "python3 <<'PY' > output.txt\nprint('x')\nPY",
        "python3 <<'PY' >> output.txt\nprint('x')\nPY",
    ] {
        let report = analyze(source)?;
        assert_eq!(report.sources.len(), 2);
        assert!(gap(&report, "Redirection effects"));
    }
    let report = analyze("cat <<'PY'\nopen('a','w')\nPY")?;
    assert_eq!(report.sources.len(), 1);
    assert!(
        report
            .operations
            .iter()
            .all(|operation| matches!(operation.effect, Effect::ProcessRun { .. }))
    );
    Ok(())
}

#[test]
fn quoted_paths_are_distinct_from_tilde_and_brace_expansions() -> eyre::Result<()> {
    for source in ["cat ~/a", "cat {a,b}"] {
        let report = analyze(source)?;
        assert_eq!(reads(&report), vec![]);
        assert!(gap(&report, "Command effects"));
    }
    for (source, path) in [("cat '~/a'", "~/a"), ("cat '{a,b}'", "{a,b}")] {
        let report = analyze(source)?;
        assert_eq!(reads(&report), vec![(path, Some("/workspace"))]);
    }
    Ok(())
}

#[test]
fn inline_environment_assignments_keep_source_context_uncertain() -> eyre::Result<()> {
    for source in [
        "PYTHONPATH=/other python3 -c 'open(\"a\",\"w\")'",
        "NODE_OPTIONS='--require ./hook.js' node -e 'require(\"fs\").unlinkSync(\"a\")'",
        "PYTHONPATH=/other python3 -c 'open(\"/absolute\",\"w\")'",
        "NODE_OPTIONS='--require ./hook.js' node -e 'require(\"fs\").unlinkSync(\"/absolute\")'",
    ] {
        let report = analyze(source)?;
        assert!(gap(&report, "runtime context"));
        assert_eq!(report.sources.len(), 1);
        assert!(
            report
                .operations
                .iter()
                .all(|operation| matches!(operation.effect, Effect::ProcessRun { .. })),
            "{report:?}"
        );
    }
    Ok(())
}

#[test]
fn unresolved_shell_definitions_do_not_reassert_interpreter_identity() -> eyre::Result<()> {
    for prefix in [
        "python3(){ :; };",
        "alias python3=':';",
        "source environment.sh;",
        "command source environment.sh;",
        "builtin source environment.sh;",
    ] {
        let report = analyze(&format!("{prefix} python3 -c 'open(\"a\",\"w\")'"))?;
        assert_eq!(report.sources.len(), 1);
        assert!(
            report
                .operations
                .iter()
                .all(|operation| matches!(operation.effect, Effect::ProcessRun { .. }))
        );
        assert!(gap(&report, "Executable identity"));
    }
    let report = analyze("CDPATH=/elsewhere cd child; cat a")?;
    assert!(reads(&report).iter().all(|(_, cwd)| cwd.is_none()));
    assert!(gap(&report, "Directory execution context"));
    let report = analyze("command cd /elsewhere; cat a")?;
    assert_eq!(reads(&report), vec![]);
    assert!(gap(&report, "Executable identity"));
    Ok(())
}

#[test]
fn deno_eval_options_belong_to_the_eval_subcommand() -> eyre::Result<()> {
    for prefix in ["deno eval", "deno eval -p", "deno eval --print"] {
        let report = analyze(&format!("{prefix} 'Deno.removeSync(\"a\")'"))?;
        assert_eq!(report.sources.len(), 2);
        assert!(report.operations.iter().any(|operation| matches!(&operation.effect, Effect::FileDelete {target: Target::Literal {path, ..}} if path == "a")));
    }
    let report = analyze("deno -e 'Deno.removeSync(\"a\")'")?;
    assert_eq!(report.sources.len(), 1);
    assert!(
        report
            .operations
            .iter()
            .all(|operation| matches!(operation.effect, Effect::ProcessRun { .. }))
    );
    assert!(gap(&report, "Command effects"));
    Ok(())
}
