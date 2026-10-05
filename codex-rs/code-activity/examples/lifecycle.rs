//! Runs only this fixed Python fixture in a new temporary directory.
use anyhow::Context;
use codex_code_activity::ActivityStream;
use codex_code_activity::ExecutionResult;
use codex_code_activity::ExecutionStatus;
use codex_code_activity::Language;
use codex_code_activity::LocalFiles;
use std::fs;
use std::io::Write;
use std::process::Command;

fn emit(value: &impl serde::Serialize) -> anyhow::Result<()> {
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, value)?;
    writeln!(stdout)?;
    Ok(())
}

fn main() -> Result<(), anyhow::Error> {
    let directory = tempfile::tempdir()?;
    let root = fs::canonicalize(directory.path())
        .with_context(|| format!("cannot canonicalize {}", directory.path().display()))?;
    let first = root.join("first.txt");
    fs::write(&first, "value=12\n").with_context(|| format!("cannot write {}", first.display()))?;
    let second = root.join("second.txt");
    fs::write(&second, "untouched\n")
        .with_context(|| format!("cannot write {}", second.display()))?;
    let mut files = LocalFiles::new(&root)?;
    let mut stream = ActivityStream::new(
        "demo-call".into(),
        Language::Python,
        Some(root.to_string_lossy().into_owned()),
    )?;
    let fragments = [
        "from pathlib import Path\nimport re\np=Path('first.txt')\np.read_text()\np.write_text(",
        "re.sub(r'value=(\\d+)', r'value=\\g<1>0', p.read_text()))\n",
        "Path('missing.txt').read_text()\nPath('second.txt').write_text('never')\n",
    ];
    for fragment in fragments {
        emit(&stream.append(fragment)?)?;
    }
    let prepared = stream.finish()?.prepare(&mut files, &[]);
    emit(&prepared.activity())?;
    let output = Command::new("python3")
        .args(["-c", &fragments.concat()])
        .current_dir(&root)
        .output()?;
    let status = output
        .status
        .code()
        .map_or(ExecutionStatus::Unknown, |code| ExecutionStatus::Exited {
            code,
        });
    let result = prepared.complete(
        &mut files,
        ExecutionResult::new(status, &String::from_utf8_lossy(&output.stderr)),
    );
    emit(&result)?;
    directory.close()?;
    Ok(())
}
