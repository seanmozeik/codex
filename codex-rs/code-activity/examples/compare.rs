//! Identical parse/emission benchmark source for current and original parsers.
//!
//! Copy this file into the pinned original checkout to compare the common API.
use codex_code_activity::Analyzer;
use codex_code_activity::Language;
use codex_code_activity::Request;
use serde::Deserialize;
use std::hint::black_box;
use std::io::BufRead;
use std::io::Write;
use std::time::Instant;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Input {
    call_id: String,
    language: Language,
    source: String,
    cwd: Option<String>,
}

impl Input {
    fn request(&self) -> Request {
        Request::new(
            self.call_id.as_str().into(),
            self.language,
            self.source.clone(),
            self.cwd.clone(),
        )
    }
}

fn main() -> eyre::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let path = args
        .first()
        .ok_or_else(|| eyre::eyre!("usage: compare corpus.jsonl [iterations] [warmup]"))?;
    let iterations: usize = args.get(1).map_or(Ok(30), |s| s.parse())?;
    let warmup: usize = args.get(2).map_or(Ok(3), |s| s.parse())?;
    eyre::ensure!(
        (1..=10_000).contains(&iterations) && warmup <= 1000,
        "invalid iteration count"
    );
    let mut analyzer = Analyzer::new()?;
    let mut stdout = std::io::stdout().lock();
    let inputs = std::io::BufReader::new(fs_err::File::open(path)?);
    for line in inputs.lines() {
        let input: Input = serde_json::from_str(&line?)?;
        if args.get(3).is_some_and(|argument| argument == "--reports") {
            // Untimed output for independent byte-equivalence checks; this
            // mode is never included in the comparative latency measurements.
            serde_json::to_writer(&mut stdout, &analyzer.analyze(input.request()))?;
            writeln!(stdout)?;
            continue;
        }
        for _ in 0..warmup {
            let report = analyzer.analyze(black_box(input.request()));
            black_box(serde_json::to_vec(&report)?);
        }
        let mut raw = Vec::with_capacity(iterations);
        let mut output = (0, 0, 0, 0);
        for _ in 0..iterations {
            let started = Instant::now();
            {
                let report = analyzer.analyze(black_box(input.request()));
                let json = serde_json::to_vec(&report)?;
                output = (
                    report.operations.len(),
                    report.unresolved.len(),
                    report.sources.len(),
                    json.len(),
                );
                black_box(json);
            }
            raw.push(started.elapsed().as_secs_f64() * 1e6);
        }
        serde_json::to_writer(
            &mut stdout,
            &serde_json::json!({
                "benchmarkVersion": 1, "case": input.call_id, "language": input.language,
                "inputBytes": input.source.len(), "iterations": iterations, "warmup": warmup,
                "microsecondsRaw": raw, "operations": output.0, "unresolved": output.1,
                "sources": output.2, "outputBytes": output.3,
                "scope": "owned request, reused analyzer, original Report JSON, output destruction; excludes setup/input decoding/process startup",
            }),
        )?;
        writeln!(stdout)?;
    }
    Ok(())
}
