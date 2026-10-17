use super::support::Fixture;
use super::support::TestResult;
use super::support::analyze;
use super::support::cases;
use super::support::text;
use codex_code_activity::PythonBarrierReason;
use codex_code_activity::PythonFinalState;
use codex_code_activity::SelectiveEditDetails;
use codex_code_activity::SelectiveEditDisposition;
use codex_code_activity::SourceDescriptionFallback;
use pretty_assertions::assert_eq;

#[test]
fn pinned_development_joins_materialize_exact_bytes() -> TestResult {
    let mut count = 0;
    for row in cases()?.iter().filter(|row| row["expected"] == "exact") {
        if matches!(row["id"].as_str(), Some("J03" | "J04")) {
            continue;
        }
        let fixture = Fixture::new(row)?;
        assert_eq!(
            fixture.analysis.disposition(),
            SelectiveEditDisposition::Analyzed
        );
        assert!(
            fixture.analysis.python_barriers().is_empty(),
            "{}",
            row["id"]
        );
        assert_eq!(fixture.analysis.edits().len(), 1, "{}", row["id"]);
        let preview = fixture.preview()?;
        assert_eq!(preview.before_bytes, Some(b"old".to_vec()));
        assert_eq!(
            preview.after_bytes,
            text(row, "after")?.as_bytes(),
            "{}",
            row["id"]
        );
        count += 1;
    }
    assert_eq!(count, 15);
    Ok(())
}

#[test]
fn empty_joins_remain_two_unsupported_cases_in_the_seventeen_case_gold() -> TestResult {
    let rows = cases()?;
    assert_eq!(
        rows.iter().filter(|row| row["expected"] == "exact").count(),
        17
    );
    let mut unsupported = 0;
    for row in rows
        .iter()
        .filter(|row| matches!(row["id"].as_str(), Some("J03" | "J04")))
    {
        assert_eq!(row["expected"], "exact");
        assert_eq!(row["after"], "");
        let analysis = analyze(text(row, "source")?)?;
        assert_eq!(analysis.disposition(), SelectiveEditDisposition::Analyzed);
        assert!(analysis.edits().is_empty());
        assert_eq!(analysis.python_barriers().len(), 1);
        assert_eq!(
            analysis.python_barriers()[0].reason,
            PythonBarrierReason::UnsupportedOperation
        );
        unsupported += 1;
    }
    assert_eq!(unsupported, 2);
    Ok(())
}

#[test]
fn unsupported_iterables_and_elements_stop_before_the_sink() -> TestResult {
    let mut count = 0;
    for row in cases()?
        .iter()
        .filter(|row| row["expected"] == "semanticRefusal")
    {
        let analysis = analyze(text(row, "source")?)?;
        assert_eq!(
            analysis.disposition(),
            SelectiveEditDisposition::Analyzed,
            "{}",
            row["id"]
        );
        assert!(analysis.edits().is_empty(), "{}", row["id"]);
        assert_eq!(analysis.python_barriers().len(), 1, "{}", row["id"]);
        count += 1;
    }
    assert_eq!(count, 13);
    Ok(())
}

#[test]
fn unsupported_whole_source_syntax_does_not_retain_a_prefix() -> TestResult {
    let mut count = 0;
    for row in cases()?
        .iter()
        .filter(|row| row["expected"] == "wholeSourceRefusal")
    {
        let analysis = analyze(text(row, "source")?)?;
        assert_eq!(
            analysis.disposition(),
            SelectiveEditDisposition::Unavailable(SourceDescriptionFallback::UnsupportedSource)
        );
        assert!(analysis.edits().is_empty());
        count += 1;
    }
    assert_eq!(count, 2);
    Ok(())
}

#[test]
fn a_semantic_barrier_retains_only_the_conditional_prefix() -> TestResult {
    let analysis = analyze(
        "from pathlib import Path\nPath('first').write_text('kept')\n\
         Path('result.txt').write_text(','.join(['a', unknown]))",
    )?;
    assert_eq!(analysis.disposition(), SelectiveEditDisposition::Analyzed);
    assert_eq!(analysis.edits().len(), 1);
    assert_eq!(analysis.python_barriers().len(), 1);
    let SelectiveEditDetails::Python { prediction, .. } = &analysis.edits()[0].details else {
        return Err("Python prefix expected".into());
    };
    assert_eq!(prediction.final_state, PythonFinalState::OpaqueSuccessor);
    Ok(())
}
