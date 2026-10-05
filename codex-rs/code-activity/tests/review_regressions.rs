//! Review regressions inspect source strings; none of these programs execute.
use codex_code_activity::Analyzer;
use codex_code_activity::Effect;
use codex_code_activity::Language;
use codex_code_activity::Report;
use codex_code_activity::Request;
use codex_code_activity::Target;
use pretty_assertions::assert_eq;

fn analyze(language: Language, source: &str) -> anyhow::Result<Report> {
    Ok(Analyzer::new()?.analyze(Request::new(
        "review".into(),
        language,
        source.into(),
        Some("/workspace".into()),
    )))
}

fn deletions(report: &Report) -> Vec<&Target> {
    report
        .operations
        .iter()
        .filter_map(|op| match &op.effect {
            Effect::FileDelete { target } => Some(target),
            _ => None,
        })
        .collect()
}

#[test]
fn path_operations_preserve_language_specific_filesystem_meaning() -> anyhow::Result<()> {
    for (language, source, expected) in [
        (
            Language::Python,
            "import os\nos.unlink(os.path.join('link', '..', 'file'))",
            "link/../file",
        ),
        (
            Language::Python,
            "from pathlib import Path\nPath('link', '..', 'file').unlink()",
            "link/../file",
        ),
        (
            Language::Python,
            "import os\nos.unlink(os.path.join('dir', '.', 'file', ''))",
            "dir/./file/",
        ),
        (
            Language::Python,
            "from pathlib import Path\nPath('//host', '.', 'file', '').unlink()",
            "//host/file",
        ),
        (
            Language::Python,
            "import os\nos.unlink(os.path.join(''))",
            "",
        ),
        (
            Language::TypeScript,
            "const path=require('path'); require('fs').unlinkSync(path.join('link', '..', 'file'))",
            "file",
        ),
        (
            Language::TypeScript,
            "const path=require('path'); require('fs').unlinkSync(path.join('dir', 'file/'))",
            "dir/file/",
        ),
    ] {
        let report = analyze(language, source)?;
        assert_eq!(
            deletions(&report),
            vec![&Target::Literal {
                path: expected.into(),
                cwd: Some("/workspace".into()),
            }],
            "{source}"
        );
    }
    Ok(())
}

#[test]
fn unknown_comprehension_code_does_not_restore_prior_bindings() -> anyhow::Result<()> {
    for source in [
        "p='old'\n[mutate() for x in [1]]\nimport os\nos.unlink(p)",
        "p='old'\n[mutate() for x in [1] for y in [2]]\nimport os\nos.unlink(p)",
    ] {
        let report = analyze(Language::Python, source)?;
        assert_eq!(deletions(&report), vec![&Target::Unresolved], "{source}");
    }
    Ok(())
}

#[test]
fn branch_alternatives_inherit_condition_side_effects() -> anyhow::Result<()> {
    for (language, source) in [
        (
            Language::Python,
            "p='old'\nif mutate():\n pass\nelse:\n import os\n os.unlink(p)",
        ),
        (
            Language::TypeScript,
            "let p='old'; if (mutate()) {} else { const fs = require('fs'); fs.unlinkSync(p); }",
        ),
    ] {
        let report = analyze(language, source)?;
        // Unknown code can change both the path and runtime API bindings.
        assert!(
            deletions(&report)
                .iter()
                .all(|target| matches!(target, Target::Unresolved)),
            "{report:?}"
        );
        assert!(!report.unresolved.is_empty());
    }
    Ok(())
}

#[test]
fn python_loop_else_effects_are_inspected() -> anyhow::Result<()> {
    let report = analyze(
        Language::Python,
        "import os\nfor p in ['a']:\n os.unlink(p)\nelse:\n os.unlink('b')",
    )?;
    assert_eq!(deletions(&report).len(), 2);
    assert!(
        deletions(&report)
            .iter()
            .any(|target| matches!(target, Target::Literal {path, ..} if path == "b"))
    );
    Ok(())
}

#[test]
fn dynamic_tool_workdir_does_not_fall_back_to_parent_directory() -> anyhow::Result<()> {
    let report = analyze(
        Language::TypeScript,
        "tools.exec_command({cmd: 'cat file', workdir: selectedDirectory})",
    )?;
    assert_eq!(report.sources.len(), 2);
    assert!(report.operations.iter().any(|op| matches!(&op.effect,
        Effect::FileRead { target: Target::Literal { path, cwd: None } } if path == "file")));
    assert!(
        report
            .unresolved
            .iter()
            .any(|gap| gap.reason.contains("directory"))
    );
    Ok(())
}
