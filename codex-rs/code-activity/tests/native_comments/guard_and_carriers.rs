use super::ActivityContract;
use super::Analyzer;
use super::Basis;
use super::Coverage;
use super::Effect;
use super::Language;
use super::MockExecutor;
use super::MockPermission;
use super::PolicyDisposition;
use super::PolicyRule;
use super::Request;
use super::UiAction;
use super::analyze;
use super::assert_eq;
use super::assert_read_coordinates;
use super::read_paths;
use super::review_script;

#[test]
fn invalid_syntax_after_comment_padding_abstains_for_the_whole_source() -> anyhow::Result<()> {
    let source = format!(
        "open('earlier.txt').read()\n{}if (\n",
        "# padding\n".repeat(64)
    );
    let report = analyze(Language::Python, &source)?;
    assert_eq!(report.coverage, Coverage::Opaque);
    assert!(report.operations.is_empty());
    assert_eq!(report.unresolved.len(), 1);
    assert_eq!(
        report.unresolved[0].reason,
        "Incomplete or invalid syntax; no effects inferred"
    );
    assert_eq!(report.unresolved[0].evidence.start_byte, 0);
    assert_eq!(report.unresolved[0].evidence.end_byte, source.len());
    assert_eq!(report.unresolved[0].evidence.start_line, 1);
    Ok(())
}

#[test]
fn late_secret_read_after_comments_blocks_the_entire_mock_dispatch() -> anyhow::Result<()> {
    let source = format!(
        "open('notes.txt').read()\n{}open('.env').read()\n",
        "# inert approval hint\n".repeat(64)
    );
    let reviewed = review_script(
        &mut Analyzer::new()?,
        Request::new(
            "late-secret".into(),
            Language::Python,
            source.clone(),
            /*cwd*/ None,
        ),
    );
    assert_eq!(read_paths(reviewed.report()), ["notes.txt", ".env"]);
    assert!(reviewed.report().unresolved.is_empty());
    assert_eq!(reviewed.decision().disposition, PolicyDisposition::Block);
    assert_eq!(reviewed.decision().hits.len(), 1);
    assert_eq!(reviewed.decision().hits[0].rule, PolicyRule::ReadEnv);
    assert_eq!(reviewed.decision().hits[0].operation_id.index(), 1);
    assert_read_coordinates(
        reviewed.report(),
        &source,
        "open('.env').read()",
        /*line*/ 66,
        /*column*/ 1,
    )?;
    let mut executor = MockExecutor::default();
    executor.dispatch(&reviewed, MockPermission::Granted);
    assert_eq!(executor.calls(), 0);
    Ok(())
}

#[test]
fn shell_python_comments_keep_original_parent_and_child_span_coordinates() -> anyhow::Result<()> {
    let python = format!(
        "{}open('nested.txt').read()\n# child tail\n",
        "# child café\n".repeat(24)
    );
    let source = format!("# outer head\npython3 - <<'PY'\n{python}PY\n# outer tail\n");
    let report = analyze(Language::Shell, &source)?;
    assert_eq!(read_paths(&report), ["nested.txt"]);
    assert_eq!(report.sources.len(), 2);
    assert!(report.unresolved.is_empty(), "{:?}", report.unresolved);
    let read = report
        .operations
        .iter()
        .find(|operation| matches!(operation.effect, Effect::FileRead { .. }))
        .expect("nested read");
    assert_eq!(read.evidence.source_id.index(), 1);
    let parent = report.sources[1]
        .parent
        .as_ref()
        .expect("embedded-source parent");
    assert_eq!(parent.source_id.index(), 0);
    assert_eq!(
        parent.start_byte,
        source.find(&python).expect("original child text")
    );
    assert_eq!(&source[parent.start_byte..parent.end_byte], python);
    assert_eq!(parent.start_line, 3);
    assert_eq!(parent.start_column, 1);
    assert_read_coordinates(
        &report,
        &python,
        "open('nested.txt').read()",
        /*line*/ 25,
        /*column*/ 1,
    )?;
    let contract = ActivityContract::from_report(&report);
    let rows = contract.ui_rows();
    assert!(
        rows.iter()
            .any(|row| row.action == UiAction::Read && row.evidence.source_id.index() == 1)
    );
    assert!(rows.iter().all(|row| row.basis == Basis::StaticIntent));
    Ok(())
}
