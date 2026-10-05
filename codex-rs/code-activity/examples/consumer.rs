//! Parse a whole synthetic script before a UI/policy consumer and mock dispatch.

use codex_code_activity::ActivityContract;
use codex_code_activity::Analyzer;
use codex_code_activity::Language;
use codex_code_activity::Request;
use codex_code_activity::policy::MockExecutor;
use codex_code_activity::policy::MockPermission;
use codex_code_activity::policy::review_script;

fn main() -> anyhow::Result<()> {
    let source = "from pathlib import Path\nPath('notes.md').read_text()\nPath('.env').write_text('synthetic-placeholder')";
    let reviewed = review_script(
        &mut Analyzer::new()?,
        Request::new(
            "synthetic-demo".into(),
            Language::Python,
            source.into(),
            None,
        ),
    );
    let activity = ActivityContract::from_report(reviewed.report());
    println!("{}", serde_json::to_string_pretty(&activity)?);
    println!("{}", serde_json::to_string_pretty(&activity.ui_rows())?);
    println!("{}", serde_json::to_string_pretty(reviewed.decision())?);
    let mut executor = MockExecutor::default();
    executor.dispatch(&reviewed, MockPermission::Granted);
    println!("mock executor calls: {}", executor.calls());
    Ok(())
}
