//! Negative and lifecycle controls for the Tripwire parser port. Sources stay inert.
use codex_code_activity::ActivityStream;
use codex_code_activity::Analyzer;
use codex_code_activity::Effect;
use codex_code_activity::ExecutionResult;
use codex_code_activity::ExecutionStatus;
use codex_code_activity::FileState;
use codex_code_activity::Language;
use codex_code_activity::ObservedChange;
use codex_code_activity::Report;
use codex_code_activity::Request;
use codex_code_activity::SnapshotProvider;
use codex_code_activity::Target;
use codex_code_activity::WriteMode;
use pretty_assertions::assert_eq;
use std::path::Path;

fn analyze(language: Language, source: &str) -> anyhow::Result<Report> {
    Ok(Analyzer::new()?.analyze(Request::new(
        "control".into(),
        language,
        source.into(),
        Some("/workspace".into()),
    )))
}

#[test]
fn python_and_node_path_join_keep_their_own_absolute_component_rules() -> anyhow::Result<()> {
    for (language, source, expected) in [
        (
            Language::Python,
            "import os\nos.unlink(os.path.join('/tmp', '/protected'))",
            "/protected",
        ),
        (
            Language::TypeScript,
            "const path=require('node:path'); require('fs').unlinkSync(path.join('/tmp', '/protected'))",
            "/tmp/protected",
        ),
    ] {
        let report = analyze(language, source)?;
        let paths: Vec<_> = report
            .operations
            .iter()
            .filter_map(|op| match &op.effect {
                Effect::FileDelete {
                    target: Target::Literal { path, .. },
                } => Some(path.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(paths, vec![expected]);
    }
    Ok(())
}

#[test]
fn read_with_mutating_flag_emits_open_intent() -> anyhow::Result<()> {
    let report = analyze(
        Language::TypeScript,
        "require('fs').readFileSync('a', {flag:'w'})",
    )?;
    assert!(report.operations.iter().any(|op| matches!(
        op.effect,
        Effect::FileOpen {
            mode: WriteMode::Replace,
            ..
        }
    )));
    assert!(
        report
            .unresolved
            .iter()
            .any(|gap| gap.reason.contains("mutate"))
    );
    Ok(())
}

#[test]
fn shell_option_values_and_script_arguments_are_not_code() -> anyhow::Result<()> {
    for source in [
        "uv run --python python3 echo -c 'open(\"a\",\"w\")'",
        "uv run pytest python3 -c 'open(\"a\",\"w\")'",
        "python3 script.py -c 'open(\"a\",\"w\")'",
        "python3 -m tool -c 'open(\"a\",\"w\")'",
        "node --test script.js -e 'require(\"fs\").rmSync(\"a\")'",
        "command -v python3",
        "bun --cwd /workspace run test",
        "deno test --allow-read test.ts",
        "node --import tsx --test test.ts",
    ] {
        let report = analyze(Language::Shell, source)?;
        assert_eq!(report.sources.len(), 1, "{source}");
        assert!(
            report
                .operations
                .iter()
                .all(|op| matches!(op.effect, Effect::ProcessRun { .. })),
            "{source}"
        );
    }
    Ok(())
}

#[test]
fn changing_loop_alias_does_not_reuse_first_iteration_meaning() -> anyhow::Result<()> {
    let report = analyze(
        Language::Python,
        "import json, os\nf=open\nfor row in json.loads('[]'):\n f('a')\n f=os.unlink\n",
    )?;
    assert!(report.operations.is_empty());
    assert!(
        report
            .unresolved
            .iter()
            .any(|gap| gap.reason.contains("Unknown callable"))
    );
    Ok(())
}

#[test]
fn differing_branch_paths_remain_unresolved_after_merge() -> anyhow::Result<()> {
    let report = analyze(
        Language::Python,
        "p='a'\nif True:\n p='b'\nopen(p, 'w').write('x')\n",
    )?;
    assert!(report.operations.iter().any(|op| matches!(
        op.effect,
        Effect::FileWrite {
            target: Target::Unresolved,
            ..
        }
    )));
    Ok(())
}

#[test]
fn explicit_modes_remain_distinct_from_empty_output() -> anyhow::Result<()> {
    for (source, expected) in [
        (
            "require('fs').writeFileSync('a', '', 'utf8')",
            WriteMode::Replace,
        ),
        (
            "require('fs').writeFileSync('a', '', {flag:'a'})",
            WriteMode::Append,
        ),
        (
            "require('fs').appendFileSync('a', '', {flag:'w'})",
            WriteMode::Replace,
        ),
        (
            "require('fs').writeFileSync('a', '', {flag:'wx'})",
            WriteMode::ExclusiveCreate,
        ),
        (
            "require('fs').writeFileSync('a', '', {flag:'r+'})",
            WriteMode::Unknown,
        ),
        (
            "Deno.writeTextFileSync('a', '', { append: false })",
            WriteMode::Replace,
        ),
        (
            "Deno.writeTextFileSync('a', '', { append: true })",
            WriteMode::Append,
        ),
    ] {
        let report = analyze(Language::TypeScript, source)?;
        let modes: Vec<_> = report
            .operations
            .iter()
            .filter_map(|op| match op.effect {
                Effect::FileWrite { mode, .. } => Some(mode),
                _ => None,
            })
            .collect();
        assert_eq!(modes, vec![expected], "{source}");
    }
    Ok(())
}

#[test]
fn nested_child_interpreter_retains_process_and_file_intent() -> anyhow::Result<()> {
    let report = analyze(
        Language::Python,
        "import subprocess\nsubprocess.run(['node', '-e', 'require(\"fs\").unlinkSync(\"a\")'])",
    )?;
    assert_eq!(report.sources.len(), 2);
    assert!(
        report
            .operations
            .iter()
            .any(|op| matches!(op.effect, Effect::ProcessRun { .. }))
    );
    assert!(
        report
            .operations
            .iter()
            .any(|op| matches!(op.effect, Effect::FileDelete { .. }))
    );
    Ok(())
}

#[test]
fn child_options_do_not_supply_a_false_execution_context() -> anyhow::Result<()> {
    let report = analyze(
        Language::Python,
        "import subprocess\nsubprocess.run(['python3', '-c', 'open(\"a\",\"w\")'], cwd='/elsewhere')",
    )?;
    assert_eq!(report.sources.len(), 1);
    assert!(
        report
            .unresolved
            .iter()
            .any(|gap| gap.reason.contains("options"))
    );
    Ok(())
}

#[test]
fn loop_and_json_support_is_stable_across_stream_revisions() -> anyhow::Result<()> {
    let source = "import json\nfrom pathlib import Path\nfor p in ['a', 'b']:\n Path(p).write_text(json.dumps({'ok': True}), encoding='utf8')\n";
    let expected = serde_json::to_value(analyze(Language::Python, source)?)?;
    for boundary in [1, 12, 51, 89, source.len()] {
        let mut stream = ActivityStream::new(
            "control".into(),
            Language::Python,
            Some("/workspace".into()),
        )?;
        stream.append(&source[..boundary])?;
        stream.append(&source[boundary..])?;
        assert_eq!(serde_json::to_value(stream.finish()?.report())?, expected);
    }
    Ok(())
}

struct UnchangedFiles;
impl SnapshotProvider for UnchangedFiles {
    fn capture(&mut self, _: &Path, _: usize) -> FileState {
        FileState::Text("original\n".into())
    }
}

#[test]
fn deletion_and_truncation_targets_enter_capture_without_execution() -> anyhow::Result<()> {
    let mut stream = ActivityStream::new(
        "capture".into(),
        Language::Python,
        Some("/workspace".into()),
    )?;
    stream.append("import os\nos.unlink('a')\nos.truncate('b', 0)\n")?;
    let mut files = UnchangedFiles;
    let prepared = stream.finish()?.prepare(&mut files, &[]);
    assert_eq!(prepared.activity().files.len(), 2);
    let completed = prepared.complete(
        &mut files,
        ExecutionResult::new(ExecutionStatus::Unknown, "no runtime was launched"),
    );
    assert_eq!(completed.files.len(), 2);
    assert!(
        completed
            .files
            .iter()
            .all(|file| file.change == ObservedChange::Unchanged && file.operation_ids.len() == 1)
    );
    assert_eq!(completed.execution.status, ExecutionStatus::Unknown);
    Ok(())
}
