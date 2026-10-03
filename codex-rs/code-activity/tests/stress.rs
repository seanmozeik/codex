//! Scaling, determinism and inert-input checks independent of timing thresholds.

#[path = "support/benchmark.rs"]
mod dataset;

use codex_code_activity::Analyzer;
use codex_code_activity::Basis;
use codex_code_activity::Coverage;
use codex_code_activity::Effect;
use codex_code_activity::Language;
use codex_code_activity::MAX_OPERATIONS;
use codex_code_activity::MAX_SOURCE_BYTES;
use codex_code_activity::Request;
use codex_code_activity::Target;
use pretty_assertions::assert_eq;

#[test]
fn scaling_corpus_remains_deterministic_and_report_counts_bounded() -> eyre::Result<()> {
    let mut analyzer = Analyzer::new()?;
    for case in dataset::cases()? {
        let first = analyzer.analyze(case.request());
        let second = analyzer.analyze(case.request());
        assert_eq!(
            serde_json::to_vec(&first)?,
            serde_json::to_vec(&second)?,
            "{}",
            case.id
        );
        assert!(first.operations.len() <= MAX_OPERATIONS, "{}", case.id);
        assert!(first.unresolved.len() <= MAX_OPERATIONS, "{}", case.id);
        assert!(first.sources.len() <= 64, "{}", case.id);
        assert!(first.diffs.is_empty(), "{}", case.id);
        assert!(
            first
                .operations
                .iter()
                .all(|op| op.basis == Basis::StaticIntent),
            "{}",
            case.id
        );
        if case.source.len() > MAX_SOURCE_BYTES {
            assert!(first.operations.is_empty(), "{}", case.id);
            assert!(
                first
                    .unresolved
                    .iter()
                    .any(|gap| gap.reason.contains("budget")),
                "{}",
                case.id
            );
        }
    }
    Ok(())
}

#[test]
fn small_representative_programs_preserve_expected_action_multiplicity() -> eyre::Result<()> {
    let mut analyzer = Analyzer::new()?;
    for case in dataset::cases()?
        .into_iter()
        .filter(|case| case.scale <= 16)
    {
        if let Some(effects) = case.effects_per_unit {
            let report = analyzer.analyze(case.request());
            assert_eq!(
                report.operations.len(),
                effects * case.scale,
                "{}: {report:?}",
                case.id
            );
        }
    }
    Ok(())
}

#[test]
fn recursive_malformed_and_callback_limit_inputs_abstain_explicitly() -> eyre::Result<()> {
    let mut analyzer = Analyzer::new()?;
    for case in dataset::cases()?
        .into_iter()
        .filter(|case| case.scale <= 129)
    {
        if let Some(expected) = &case.required_gap {
            let report = analyzer.analyze(case.request());
            assert!(
                report
                    .unresolved
                    .iter()
                    .any(|gap| gap.reason.contains(expected)),
                "{}: {report:?}",
                case.id
            );
            assert!(matches!(
                report.coverage,
                Coverage::Partial | Coverage::Opaque
            ));
        }
    }
    Ok(())
}

#[test]
fn source_programs_cannot_write_a_real_host_sentinel() -> eyre::Result<()> {
    let directory = tempfile::tempdir()?;
    let sentinel = directory.path().join("sentinel.txt");
    fs_err::write(&sentinel, "unchanged")?;
    let quoted_path = serde_json::to_string(&sentinel.to_string_lossy())?;
    let mut analyzer = Analyzer::new()?;
    for (language, source) in [
        (
            Language::Python,
            format!("open({quoted_path}, 'w').write('executed')"),
        ),
        (
            Language::TypeScript,
            format!("require('fs').writeFileSync({quoted_path}, 'executed')"),
        ),
        (Language::Shell, format!("printf executed > {quoted_path}")),
    ] {
        let report = analyzer.analyze(Request::new("inert".into(), language, source, None));
        assert!(!report.operations.is_empty(), "{report:?}");
        assert_eq!(fs_err::read_to_string(&sentinel)?, "unchanged");
    }
    Ok(())
}

#[test]
fn dynamic_targets_remain_explicitly_unresolved_in_the_scaling_corpus() -> eyre::Result<()> {
    let mut analyzer = Analyzer::new()?;
    for case in dataset::cases()?
        .into_iter()
        .filter(|case| case.id.starts_with("python-dynamic-target-"))
    {
        let report = analyzer.analyze(case.request());
        assert!(!report.operations.is_empty(), "{}", case.id);
        assert!(
            report.operations.iter().all(|op| matches!(
                &op.effect,
                Effect::FileOpen {
                    target: Target::Unresolved,
                    ..
                } | Effect::FileWrite {
                    target: Target::Unresolved,
                    ..
                }
            )),
            "{}: {report:?}",
            case.id
        );
    }
    Ok(())
}

#[test]
fn repeated_awaited_tools_preserve_all_child_sources_before_source_caps() -> eyre::Result<()> {
    let case = dataset::cases()?
        .into_iter()
        .find(|case| case.id == "typescript-tools-shell-python-8")
        .ok_or_else(|| eyre::eyre!("missing nested-tools fixture"))?;
    let report = Analyzer::new()?.analyze(case.request());
    assert_eq!(report.sources.len(), 17, "{report:?}");
    assert_eq!(report.operations.len(), 16, "{report:?}");
    for source in report.sources.iter().skip(1) {
        assert!(source.parent.is_some(), "{source:?}");
    }
    Ok(())
}

#[test]
fn unawaited_promise_content_never_establishes_a_text_edit() -> eyre::Result<()> {
    let mut analyzer = Analyzer::new()?;
    for case in dataset::cases()?
        .into_iter()
        .filter(|case| case.id.starts_with("javascript-unawaited-promises-"))
    {
        let report = analyzer.analyze(case.request());
        assert!(
            report
                .operations
                .iter()
                .any(|op| matches!(op.effect, Effect::FileRead { .. })),
            "{}",
            case.id
        );
        assert!(
            !report
                .operations
                .iter()
                .any(|op| matches!(op.effect, Effect::FileEdit { .. })),
            "{}: {report:?}",
            case.id
        );
        assert!(!report.unresolved.is_empty(), "{}", case.id);
    }
    Ok(())
}

#[test]
fn a_single_recursive_call_retains_only_unresolved_target_intent() -> eyre::Result<()> {
    let mut analyzer = Analyzer::new()?;
    for (name, count) in [("python-recursion-1", 2), ("typescript-recursion-1", 1)] {
        let case = dataset::cases()?
            .into_iter()
            .find(|case| case.id == name)
            .ok_or_else(|| eyre::eyre!("missing recursion fixture {name}"))?;
        let report = analyzer.analyze(case.request());
        assert_eq!(report.operations.len(), count, "{name}: {report:?}");
        assert!(
            report.operations.iter().all(|op| matches!(
                &op.effect,
                Effect::FileOpen {
                    target: Target::Unresolved,
                    ..
                } | Effect::FileWrite {
                    target: Target::Unresolved,
                    ..
                } | Effect::FileDelete {
                    target: Target::Unresolved
                }
            )),
            "{name}: {report:?}"
        );
        assert!(
            report
                .unresolved
                .iter()
                .any(|gap| gap.reason.contains("Recursive")),
            "{name}: {report:?}"
        );
    }
    Ok(())
}
