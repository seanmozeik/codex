//! JSONL command-line adapter for complete source analysis.

use codex_code_activity::Analyzer;
use codex_code_activity::Request;
use eyre::WrapErr;
use std::io;
use std::io::BufRead;
use std::io::Write;

fn main() -> eyre::Result<()> {
    let mut analyzer = Analyzer::new().wrap_err("cannot initialize source analysis")?;
    let stdin = io::stdin();
    let mut stdout = io::BufWriter::new(io::stdout().lock());
    for (index, line) in stdin.lock().lines().enumerate() {
        let line = line.wrap_err_with(|| format!("cannot read request line {}", index + 1))?;
        if line.trim().is_empty() {
            continue;
        }
        let request: Request = serde_json::from_str(&line)
            .wrap_err_with(|| format!("cannot decode request line {}", index + 1))?;
        serde_json::to_writer(&mut stdout, &analyzer.analyze(request))?;
        writeln!(stdout)?;
        stdout.flush()?;
    }
    Ok(())
}
