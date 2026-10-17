//! Serial complete-request boundary benchmark. Input sources are never executed.
#[path = "selective_support/mod.rs"]
mod selective_boundary;

use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::CompleteSourceDescriptionInput;
use codex_code_activity::Request;
use std::hint::black_box;
use std::io::BufRead;
use std::io::Write;
use std::time::Instant;

#[derive(Clone, Copy, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
enum ProbeEnvironment {
    #[default]
    FreshProcess,
    PersistentOrUnknown,
}

#[derive(serde::Deserialize)]
struct ProbeRequest {
    #[serde(flatten)]
    request: Request,
    #[serde(default, alias = "_environment")]
    environment: ProbeEnvironment,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().nth(1).as_deref() == Some("--selective-edits") {
        return selective_boundary::run();
    }
    if std::env::args().nth(1).as_deref() == Some("--selective-benchmark") {
        return selective_boundary::benchmark();
    }
    let lines = std::io::stdin()
        .lock()
        .lines()
        .collect::<Result<Vec<_>, _>>()?;
    let rounds: usize = std::env::var("BENCH_ROUNDS")
        .unwrap_or_else(|_| "9".into())
        .parse()?;
    let repeats = rounds.max(1);
    let mut times = vec![Vec::new(); lines.len()];
    let mut outputs = vec![Vec::new(); lines.len()];
    let started = Instant::now();
    let mut analyzer = Analyzer::new()?;
    let construction_ns = started.elapsed().as_nanos();
    for round in 0..=repeats {
        for offset in 0..lines.len() {
            let i = (offset + round * 137) % lines.len();
            let started = Instant::now();
            let envelope: ProbeRequest = serde_json::from_str(black_box(&lines[i]))?;
            let persistent = envelope.environment == ProbeEnvironment::PersistentOrUnknown;
            let request = envelope.request;
            let _ = persistent;
            let encoded = match CompleteSourceDescriptionInput::from_source(
                request.call_id,
                request.language,
                &request.source,
                AnalysisLimits::default(),
            ) {
                Ok(input) => serde_json::to_vec(&analyzer.describe_complete(input))?,
                Err(error) => {
                    serde_json::to_vec(&serde_json::json!({"bufferError":error.to_string()}))?
                }
            };
            black_box(&encoded);
            let elapsed = started.elapsed().as_nanos();
            if round > 0 {
                times[i].push(elapsed);
            }
            if round == repeats {
                outputs[i] = encoded;
            }
        }
    }
    let mut output = std::io::BufWriter::new(std::io::stdout().lock());
    for (i, (times, bytes)) in times.into_iter().zip(outputs).enumerate() {
        let value: serde_json::Value = serde_json::from_slice(&bytes)?;
        serde_json::to_writer(
            &mut output,
            &serde_json::json!({"index":i,"constructionNs":construction_ns,"elapsedNs":times,"result":value}),
        )?;
        writeln!(output)?;
    }
    Ok(())
}
