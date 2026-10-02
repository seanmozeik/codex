//! Compile-tested boundary adapter. It has no dependency on codex-core.
//! The host provides the original argv from `ExecCommandBeginEvent`, not a display string.
use codex_code_activity::Analyzer;
use codex_code_activity::Language;
use codex_code_activity::Request;
use std::io::Write;

fn main() -> Result<(), eyre::Report> {
    let argv = [
        "/bin/zsh",
        "-lc",
        "python3 - <<'PY'\nfrom pathlib import Path\np=Path('settings.ts')\np.write_text(p.read_text().replace('old', 'new'))\nPY",
    ];
    // A production host should use codex-shell-command::extract_shell_command.
    let shell_source = argv[2];
    let report = Analyzer::new()?.analyze(Request::new(
        "example-command-id".into(),
        Language::Shell,
        shell_source.into(),
        Some("/workspace".into()),
    ));
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer_pretty(&mut stdout, &report)?;
    writeln!(stdout)?;
    Ok(())
}
