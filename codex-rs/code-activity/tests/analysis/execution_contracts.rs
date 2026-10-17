use super::Analyzer;
use super::FileDiff;
use super::FileSnapshot;
use super::Language;
use super::Request;
use super::assert_eq;
use super::run;

#[test]
fn snapshots_diff_noop_and_partial_failure_are_independent_of_intent() -> anyhow::Result<()> {
    let mut request = Request::new(
        "diff".into(),
        Language::Python,
        "raise RuntimeError('later failure')".into(),
        /*cwd*/ None,
    )
    .with_snapshots(vec![
        FileSnapshot::new("a".into(), Some("old\n".into()), Some("new\n".into())),
        FileSnapshot::new("b".into(), Some("same\n".into()), Some("same\n".into())),
    ]);
    let r = Analyzer::new()?.analyze(request);
    assert!(
        matches!(&r.diffs[0],FileDiff::Compared {changed:true,unified_diff,..} if unified_diff.contains("-old\n+new"))
    );
    assert!(
        matches!(&r.diffs[1],FileDiff::Compared {changed:false,unified_diff,..} if unified_diff.is_empty())
    );
    request = Request::new(
        "empty".into(),
        Language::Python,
        String::new(),
        /*cwd*/ None,
    )
    .with_snapshots(vec![FileSnapshot::new(
        "empty".into(),
        /*before*/ None,
        Some(String::new()),
    )]);
    assert!(matches!(
        &Analyzer::new()?.analyze(request).diffs[0],
        FileDiff::Compared { changed: true, .. }
    ));
    Ok(())
}
#[test]
fn sessions_do_not_share_bindings() -> anyhow::Result<()> {
    let mut a = Analyzer::new()?;
    let _ = a.analyze(Request::new(
        "1".into(),
        Language::Python,
        "from pathlib import Path\np=Path('a')".into(),
        /*cwd*/ None,
    ));
    let r = a.analyze(Request::new(
        "2".into(),
        Language::Python,
        "p.write_text('x')".into(),
        /*cwd*/ None,
    ));
    assert!(r.operations.is_empty());
    Ok(())
}
#[test]
fn source_and_operation_budgets() -> anyhow::Result<()> {
    let r = run(
        Language::Python,
        &" ".repeat(codex_code_activity::MAX_SOURCE_BYTES + 1),
    )?;
    assert!(r.operations.is_empty());
    let r = run(Language::Python, &"open('a').read()\n".repeat(300))?;
    assert_eq!(r.operations.len(), codex_code_activity::MAX_OPERATIONS);
    assert!(!r.unresolved.is_empty());
    Ok(())
}

#[test]
fn expanding_constants_and_arrays_abstain_before_growth() -> anyhow::Result<()> {
    for source in [
        format!("s='x'\n{}", "s=s+s\n".repeat(80)),
        format!("a=['x']\n{}", "a=[a,a]\n".repeat(80)),
    ] {
        let r = run(Language::Python, &source)?;
        assert!(
            r.unresolved
                .iter()
                .any(|u| u.reason.contains("Abstract value construction budget"))
        );
    }
    Ok(())
}

#[test]
fn omitted_snapshot_inputs_remain_visible() -> anyhow::Result<()> {
    let r = Analyzer::new()?.analyze(
        Request::new(
            "bounded".into(),
            Language::Python,
            String::new(),
            /*cwd*/ None,
        )
        .with_snapshots(
            (0..257)
                .map(|i| {
                    FileSnapshot::new(i.to_string(), /*before*/ None, /*after*/ None)
                })
                .collect(),
        ),
    );
    assert_eq!(
        r.diffs.last(),
        Some(&FileDiff::Truncated { omitted_files: 1 })
    );
    Ok(())
}
