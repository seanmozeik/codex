//! runtime behavior tests.
use super::*;
use pretty_assertions::assert_eq;

#[test]
#[ignore = "requires python3; executes only the fixed test fixture"]
fn real_python_regex_edit_survives_later_process_failure() -> anyhow::Result<()> {
    let work = Workspace::new()?;
    work.file("first", "value=12\n")?;
    work.file("second", "untouched\n")?;
    let source = "from pathlib import Path\nimport re\np=Path('first')\np.write_text(re.sub(r'value=(\\d+)', r'value=\\g<1>0', p.read_text()))\nPath('missing').read_text()\nPath('second').write_text('never')\n";
    let prepared = work.prepare(source)?;
    let output = std::process::Command::new("python3")
        .args(["-c", source])
        .current_dir(&work.root)
        .output()?;
    let result = prepared.complete(
        &mut work.provider()?,
        ExecutionResult::new(
            ExecutionStatus::Exited {
                code: output
                    .status
                    .code()
                    .ok_or_else(|| anyhow::anyhow!("process has no exit code"))?,
            },
            &String::from_utf8_lossy(&output.stderr),
        ),
    );
    assert_eq!(result.execution.status, ExecutionStatus::Exited { code: 1 });
    assert_eq!(result.files.len(), 3);
    assert_eq!(result.files[0].change, ObservedChange::Modified);
    assert_eq!(result.files[1].change, ObservedChange::Unchanged);
    assert_eq!(result.files[2].change, ObservedChange::Unchanged);
    assert!(result.execution.output().contains("FileNotFoundError"));
    let json = serde_json::to_string(&result)?;
    assert!(json.contains("+value=120"));
    assert!(json.contains("regexSubstitution"));
    Ok(())
}

#[test]
#[ignore = "requires node; executes only the fixed test fixture"]
fn real_javascript_regex_callback_uses_observed_content() -> anyhow::Result<()> {
    let work = Workspace::new()?;
    work.file("a", "value=12\n")?;
    let source = "import fs from 'node:fs';\nconst p='a';\nfs.writeFileSync(p, fs.readFileSync(p, 'utf8').replace(/value=(\\d+)/g, (_, n) => 'value=' + (Number(n) + 1)));\n";
    let mut stream = work.stream(Language::TypeScript)?;
    stream.append(source)?;
    let prepared = stream.finish()?.prepare(&mut work.provider()?, &[]);
    let output = std::process::Command::new("node")
        .args(["--input-type=module", "-e", source])
        .current_dir(&work.root)
        .output()?;
    let result = prepared.complete(
        &mut work.provider()?,
        ExecutionResult::new(
            ExecutionStatus::Exited {
                code: output
                    .status
                    .code()
                    .ok_or_else(|| anyhow::anyhow!("process has no exit code"))?,
            },
            &String::from_utf8_lossy(&output.stderr),
        ),
    );
    assert_eq!(result.execution.status, ExecutionStatus::Exited { code: 0 });
    assert_eq!(result.files[0].change, ObservedChange::Modified);
    assert!(serde_json::to_string(&result)?.contains("+value=13"));
    Ok(())
}
