//! streaming behavior tests.
use super::*;
use pretty_assertions::assert_eq;

#[test]
fn incomplete_call_preserves_prior_intent_then_finishes() -> eyre::Result<()> {
    let mut stream = ActivityStream::new("s".into(), Language::Python, Some("/work".into()))?;
    let first =
        stream.append("from pathlib import Path\np=Path('a')\np.read_text()\np.write_text(")?;
    assert!(!first.syntax_complete);
    assert_eq!(first.report.operations.len(), 1);
    assert!(matches!(
        first.report.operations[0].effect,
        Effect::FileRead { .. }
    ));
    let next = stream.append("'new')\n")?;
    assert_eq!(next.revision.get(), first.revision.get() + 1);
    assert!(next.syntax_complete);
    assert_eq!(next.report.operations.len(), 2);
    assert_eq!(stream.finish()?.report(), &next.report);
    Ok(())
}

#[test]
fn later_tokens_can_retract_a_valid_preview() -> eyre::Result<()> {
    let mut stream = ActivityStream::new("s".into(), Language::Python, None)?;
    let pending = stream.append("open('a','w').write('x')")?;
    assert!(!pending.report.operations.is_empty());
    let revised = stream.append(" if False else None")?;
    assert!(revised.syntax_complete);
    assert!(revised.report.operations.is_empty());
    assert!(stream.finish()?.report().operations.is_empty());
    Ok(())
}

#[test]
fn incomplete_final_call_retracts_all_pending_intent() -> eyre::Result<()> {
    let mut stream = ActivityStream::new("s".into(), Language::Python, None)?;
    assert!(
        !stream
            .append("open('a').read()\nopen(")?
            .report
            .operations
            .is_empty()
    );
    assert!(stream.finish()?.report().operations.is_empty());
    Ok(())
}

#[test]
fn every_utf8_fragment_boundary_produces_the_same_final_analysis() -> eyre::Result<()> {
    for (language, source) in [
        (
            Language::Python,
            "from pathlib import Path\np=Path('café.txt')\np.write_text(p.read_text().replace('旧','新'))\n",
        ),
        (
            Language::TypeScript,
            "import fs from 'node:fs';\nconst p = 'café.txt';\nfs.writeFileSync(p, fs.readFileSync(p, 'utf8').replace(/old/g, 'new'));\n",
        ),
        (
            Language::Shell,
            "python3 - <<'PY'\nfrom pathlib import Path\nPath('café').write_text('新')\nPY\n",
        ),
    ] {
        let expected =
            Analyzer::new()?.analyze(Request::new("s".into(), language, source.into(), None));
        let mut stream = ActivityStream::new("s".into(), language, None)?;
        for character in source.chars() {
            let pending = stream.append(&character.to_string())?;
            for op in &pending.report.operations {
                if op.evidence.source_id.index() == 0 {
                    assert!(op.evidence.end_byte <= pending.analyzed_bytes);
                }
            }
        }
        assert_eq!(stream.finish()?.report(), &expected);
    }
    Ok(())
}

#[test]
fn oversized_stream_cannot_be_prepared_from_a_retained_prefix() -> eyre::Result<()> {
    let mut stream = ActivityStream::new("s".into(), Language::Python, None)?;
    stream.append("open('a','w')")?;
    assert!(stream.append(&"x".repeat(MAX_SOURCE_BYTES)).is_err());
    assert!(stream.finish().is_err());
    Ok(())
}

#[test]
fn terminal_result_adapter_rejects_running_sessions_and_stdout_status_guesses() -> eyre::Result<()>
{
    let result =
        ExecutionResult::from_exec_command_json(r#"{"exit_code":1,"output":"I succeeded"}"#)?;
    assert_eq!(result.status, ExecutionStatus::Exited { code: 1 });
    assert!(
        ExecutionResult::from_exec_command_json(
            r#"{"session_id":42,"exit_code":null,"output":"exit code 0"}"#
        )
        .is_err()
    );
    assert!(ExecutionResult::from_exec_command_json(r#"{"output":"exit code 1"}"#).is_err());
    let bounded = ExecutionResult::new(ExecutionStatus::Unknown, &"😃".repeat(3000));
    assert!(bounded.output_truncated());
    assert!(bounded.output().len() <= 8192);
    Ok(())
}
