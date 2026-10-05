//! Regression tests for source interpretation and supplied diffs.

use pretty_assertions::assert_eq;

use codex_code_activity::Analyzer;
use codex_code_activity::Basis;
use codex_code_activity::Effect;
use codex_code_activity::FileDiff;
use codex_code_activity::FileSnapshot;
use codex_code_activity::Language;
use codex_code_activity::Request;
use codex_code_activity::Target;
use codex_code_activity::WriteMode;

fn run(language: Language, source: &str) -> anyhow::Result<codex_code_activity::Report> {
    Ok(Analyzer::new()?.analyze(Request::new(
        "test".into(),
        language,
        source.into(),
        Some("/workspace".into()),
    )))
}
fn edit_targets(r: &codex_code_activity::Report) -> Vec<&Target> {
    r.operations
        .iter()
        .filter_map(|o| match &o.effect {
            Effect::FileEdit { target, .. } => Some(target),
            _ => None,
        })
        .collect()
}
fn path(p: &str) -> Target {
    Target::Literal {
        path: p.into(),
        cwd: Some("/workspace".into()),
    }
}

#[test]
fn python_alias_and_chained_edit() -> anyhow::Result<()> {
    let r = run(
        Language::Python,
        "from pathlib import Path as P\np=P('config.ts')\np.write_text(p.read_text().replace('old', 'new'))\n",
    )?;
    assert_eq!(edit_targets(&r), vec![&path("config.ts")]);
    assert_eq!(r.operations.len(), 2);
    assert!(r.operations.iter().all(|o| o.basis == Basis::StaticIntent));
    Ok(())
}
#[test]
fn builtin_open_tracks_dataflow() -> anyhow::Result<()> {
    let r = run(
        Language::Python,
        "p='file.ts'\ns=open(p).read()\ns=s.replace('old','new',1)\nopen(p,'w').write(s)\n",
    )?;
    assert_eq!(edit_targets(&r), vec![&path("file.ts")]);
    Ok(())
}
#[test]
fn regex_intent_is_not_a_computed_diff() -> anyhow::Result<()> {
    let r = run(
        Language::Python,
        "import re\ns=open('a').read()\ns=re.sub(r'(old)+',r'new',s)\nopen('a','w').write(s)\n",
    )?;
    assert_eq!(edit_targets(&r), vec![&path("a")]);
    assert!(r.diffs.is_empty());
    Ok(())
}

#[test]
fn python_slice_splice_preserves_origin_without_evaluating_indices() -> anyhow::Result<()> {
    let r = run(
        Language::Python,
        "s=open('a').read()\ns=s[:start]+'inserted'+s[end:]\nopen('a','w').write(s)\n",
    )?;
    assert_eq!(edit_targets(&r), vec![&path("a")]);
    assert!(r.diffs.is_empty());
    Ok(())
}
#[test]
fn with_handles_and_append_mode() -> anyhow::Result<()> {
    let r = run(
        Language::Python,
        "with open('a','a') as f:\n f.write('line')\n",
    )?;
    assert!(r.operations.iter().any(|o| o.effect
        == Effect::FileWrite {
            target: path("a"),
            mode: WriteMode::Append
        }));
    Ok(())
}
#[test]
fn different_destination_is_write_not_edit() -> anyhow::Result<()> {
    let r = run(
        Language::Python,
        "s=open('a').read().replace('x','y')\nopen('b','w').write(s)\n",
    )?;
    assert!(edit_targets(&r).is_empty());
    assert!(r.operations.iter().any(|o| o.effect
        == Effect::FileWrite {
            target: path("b"),
            mode: WriteMode::Replace
        }));
    Ok(())
}
#[test]
fn comments_and_strings_are_not_calls() -> anyhow::Result<()> {
    let r = run(
        Language::Python,
        "# open('secret','w').write('x')\nprint(\"Path('secret').read_text()\")\n",
    )?;
    assert!(r.operations.is_empty());
    Ok(())
}
#[test]
fn shadowed_builtin_is_not_file_access() -> anyhow::Result<()> {
    let r = run(Language::Python, "open = custom\nopen('a').read()\n")?;
    assert!(r.operations.is_empty());
    assert!(!r.unresolved.is_empty());
    Ok(())
}
#[test]
fn unknown_call_invalidates_known_paths() -> anyhow::Result<()> {
    let r = run(
        Language::Python,
        "from pathlib import Path\np=Path('a')\nchange_globals()\np.write_text('x')\n",
    )?;
    assert!(r.operations.is_empty());
    Ok(())
}
#[test]
fn uninvoked_functions_do_not_emit_edits() -> anyhow::Result<()> {
    let r = run(Language::Python, "def f():\n open('a','w').write('x')\n")?;
    assert!(r.operations.is_empty());
    // The definition is now understood; its body is inspected only on a call.
    assert_eq!(r.coverage, codex_code_activity::Coverage::Opaque);
    Ok(())
}
#[test]
fn branch_operations_are_conditional_intent() -> anyhow::Result<()> {
    let r = run(Language::Python, "if False:\n open('a','w').write('x')\n")?;
    assert_eq!(r.operations.len(), 2);
    assert!(r.operations.iter().all(|o| o.basis == Basis::StaticIntent));
    assert!(
        r.unresolved
            .iter()
            .any(|g| g.reason.contains("conditional"))
    );
    Ok(())
}
#[test]
fn subprocess_alias_has_argv_and_no_execution_claim() -> anyhow::Result<()> {
    let r = run(
        Language::Python,
        "import subprocess as sp\nsp.run(['bash','-lc','git status --short'],check=True)\n",
    )?;
    assert_eq!(
        r.operations[0].effect,
        Effect::ProcessRun {
            argv: vec![
                Some("bash".into()),
                Some("-lc".into()),
                Some("git status --short".into())
            ],
            shell: None
        }
    );
    Ok(())
}
#[test]
fn typescript_import_alias_and_await() -> anyhow::Result<()> {
    let r = run(
        Language::TypeScript,
        "import {readFile as read, writeFile} from 'node:fs/promises'; const p: string = 'a.ts'; const s = await read(p, 'utf8'); await writeFile(p, s.replace(/old/g, 'new'));",
    )?;
    assert_eq!(edit_targets(&r), vec![&path("a.ts")]);
    Ok(())
}
#[test]
fn typescript_namespace_and_dynamic_import() -> anyhow::Result<()> {
    for source in [
        "import * as fs from 'node:fs'; fs.writeFileSync('a', fs.readFileSync('a', 'utf8').replace('x','y'));",
        "const fs = await import('node:fs/promises'); const s = await fs.readFile('a','utf8'); await fs.writeFile('a',s.replaceAll('x','y'));",
    ] {
        assert_eq!(
            edit_targets(&run(Language::TypeScript, source)?),
            vec![&path("a")]
        );
    }
    Ok(())
}
#[test]
fn typescript_subprocess_and_bun() -> anyhow::Result<()> {
    let r = run(
        Language::TypeScript,
        "import {spawn} from 'node:child_process'; spawn('git',['status','--short']); const s = await Bun.file('a').text(); await Bun.write('a',s.replace('x','y'));",
    )?;
    assert_eq!(edit_targets(&r), vec![&path("a")]);
    assert!(matches!(r.operations[0].effect, Effect::ProcessRun { .. }));
    Ok(())
}
#[test]
fn nested_codex_envelope_unwraps_both_languages() -> anyhow::Result<()> {
    let source = r#"text(await tools.exec_command({cmd: "python3 - <<'PY'\np='a';s=open(p).read();open(p,'w').write(s.replace('x','y'))\nPY",workdir:'/repo'}));"#;
    let r = run(Language::TypeScript, source)?;
    assert_eq!(r.sources.len(), 3);
    assert_eq!(
        edit_targets(&r),
        vec![&Target::Literal {
            path: "a".into(),
            cwd: Some("/repo".into())
        }]
    );
    assert!(r.sources[2].parent.is_some());
    Ok(())
}
#[test]
fn heredoc_written_as_data_is_not_executed() -> anyhow::Result<()> {
    let r = run(
        Language::Shell,
        "cat <<'PY' > example.py\nopen('secret','w').write('x')\nPY\n",
    )?;
    assert!(edit_targets(&r).is_empty());
    assert!(
        !r.operations
            .iter()
            .any(|o| matches!(o.effect, Effect::FileWrite { .. }))
    );
    Ok(())
}
#[test]
fn unquoted_heredoc_is_unresolved() -> anyhow::Result<()> {
    let r = run(
        Language::Shell,
        "python3 - <<PY\nopen('$FILE','w').write('x')\nPY\n",
    )?;
    assert!(edit_targets(&r).is_empty());
    assert_eq!(r.sources.len(), 1);
    Ok(())
}
#[test]
fn syntax_errors_do_not_produce_partial_claims() -> anyhow::Result<()> {
    let r = run(Language::Python, "open('a').read(\n")?;
    assert!(r.operations.is_empty());
    assert_eq!(r.unresolved.len(), 1);
    Ok(())
}
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
