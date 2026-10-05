//! Local prototype: actual Codex core item plus static intent; source never runs.
//! No core dispatch, app-server notification or UI has been wired to this adapter.

use codex_code_activity::Analyzer;
use codex_code_activity::analyze_codex_command;
use codex_protocol::items::CommandExecutionItem;
use codex_protocol::items::CommandExecutionStatus;
use codex_protocol::protocol::ExecCommandSource;
use codex_shell_command::parse_command::parse_command;
use codex_utils_path_uri::PathUri;
use std::io::Write;

fn main() -> anyhow::Result<()> {
    let command = [
        "/bin/zsh",
        "-lc",
        "python3 - <<'PY'\nfrom pathlib import Path\np=Path('settings.ts')\np.write_text(p.read_text().replace('old', 'new'))\nPY",
    ]
    .map(str::to_owned)
    .to_vec();
    let item = CommandExecutionItem {
        sandbox_type: None,
        model_context: None,
        id: "prototype-command".into(),
        plugin_id: None,
        script_path: None,
        process_id: None,
        parsed_cmd: parse_command(&command),
        command,
        cwd: PathUri::parse("file:///workspace")?,
        source: ExecCommandSource::Agent,
        interaction_input: None,
        status: CommandExecutionStatus::InProgress,
        aggregated_output: None,
        exit_code: None,
        duration: None,
    };
    let activity = analyze_codex_command(&mut Analyzer::new()?, &item);
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer_pretty(&mut stdout, &activity)?;
    writeln!(stdout)?;
    Ok(())
}
