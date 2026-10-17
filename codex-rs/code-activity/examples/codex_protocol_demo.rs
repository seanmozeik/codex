//! Two-phase local protocol demo. It never executes the command or asserts trust.
//! The runner waits for flushed `prepared`, dispatches exactly its argv/cwd,
//! sends one actual completion, and closes stdin. Observations cover settings.txt.

#[path = "codex_protocol_demo/flow.rs"]
mod flow;
#[path = "codex_protocol_demo/input.rs"]
mod input;
#[cfg(test)]
#[path = "codex_protocol_demo/tests.rs"]
mod tests;

use input::Completion;
use input::Launch;
use serde_json::Value;
use std::io;
use std::io::Write;
use std::sync::mpsc;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
enum DemoError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Activity(#[from] codex_code_activity::ActivityError),
    #[error(transparent)]
    Capture(#[from] codex_code_activity::CaptureError),
    #[error("invalid demo protocol: {0}")]
    Protocol(&'static str),
}

type Result<T> = std::result::Result<T, DemoError>;

fn main() -> Result<()> {
    // One bounded reader per demo process; never join a potentially blocked stdin.
    // The reader handles exactly two lines and an EOF/duplicate check.
    let (sender, receiver) = mpsc::sync_channel(1);
    let _reader = std::thread::spawn(move || {
        let stdin = io::stdin();
        let mut reader = stdin.lock();
        for _ in 0..3 {
            let line = input::read_line(&mut reader);
            let terminal = !matches!(&line, Ok(Some(_)));
            if sender.send(line).is_err() || terminal {
                break;
            }
        }
    });
    let next = || {
        receiver
            .recv_timeout(Duration::from_secs(30))
            .map_err(|_| DemoError::Protocol("input deadline or reader failure"))?
    };
    let launch: Launch =
        serde_json::from_slice(&next()?.ok_or(DemoError::Protocol("EOF before launch"))?)?;
    let (prepared, output) = flow::prepare(launch)?;
    write_line(&output)?;
    let completion: Completion =
        serde_json::from_slice(&next()?.ok_or(DemoError::Protocol("EOF before completion"))?)?;
    // Do not publish a terminal result until input is definitively single-use.
    if next()?.is_some() {
        return Err(DemoError::Protocol("duplicate or trailing input"));
    }
    write_line(&prepared.complete(completion)?)
}

fn write_line(value: &Value) -> Result<()> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    serde_json::to_writer(&mut output, value)?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(())
}
