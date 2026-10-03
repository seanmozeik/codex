//! Runs only this fixed Python fixture in a new temporary directory.
use codex_code_activity::ActivityStream;
use codex_code_activity::ExecutionResult;
use codex_code_activity::ExecutionStatus;
use codex_code_activity::Language;
use codex_code_activity::LocalFiles;
use fs_err as fs;
use std::io::Write;
use std::process::Command;

fn emit(value: &impl serde::Serialize) -> eyre::Result<()> {
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, value)?;
    writeln!(stdout)?;
    Ok(())
}

fn main() -> Result<(), eyre::Report> {
    let directory = tempfile::tempdir()?;
    let root = fs::canonicalize(directory.path())?;
    fs::write(root.join("first.txt"), "value=12\n")?;
    fs::write(root.join("second.txt"), "untouched\n")?;
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
