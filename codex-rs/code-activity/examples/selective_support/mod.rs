//! Inert JSON-lines capture for conditional selective edit forecasts.
//! The boundary never reads target paths, dispatches tools, or executes inputs.

use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::CallId;
use codex_code_activity::GUARD_MAX_SOURCE_BYTES;
use codex_code_activity::Language;
use codex_code_activity::PythonWriteEndpoint;
use codex_code_activity::SelectiveEditHostProfile;
use codex_code_activity::SelectiveEditInput;
use serde::Deserialize;
use serde_json::json;
use std::hint::black_box;
use std::io::BufRead;
use std::io::Read;
use std::io::Write;
use std::time::Duration;
use std::time::Instant;

const DEFAULT_BENCH_ROUNDS: usize = 9;
const MAX_BENCH_ROUNDS: usize = 100;
const MAX_BENCH_ROWS: usize = 4096;
const MAX_BENCH_LINE_BYTES: usize = 1_048_576;
const MAX_BENCH_INPUT_BYTES: usize = 64 * 1_048_576;
const MAX_BENCH_RESULT_BYTES: usize = 256 * 1_048_576;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Case {
    id: String,
    language: Language,
    source: String,
    cwd: Option<String>,
    source_limit: Option<usize>,
    host_profile: Option<SelectiveEditHostProfile>,
    #[serde(default)]
    include_source_texts: bool,
    #[serde(default)]
    python_write_endpoint: PythonWriteEndpoint,
}

pub(crate) fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut analyzer = Analyzer::new()?;
    let mut output = std::io::BufWriter::new(std::io::stdout().lock());
    for line in std::io::stdin().lock().lines() {
        let case: Case = serde_json::from_str(&line?)?;
        let record = capture_record(&mut analyzer, &case)?;
        serde_json::to_writer(&mut output, &record)?;
        writeln!(output)?;
    }
    Ok(())
}

fn capture_record(
    analyzer: &mut Analyzer,
    case: &Case,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let input = SelectiveEditInput::from_source(
        CallId::from(case.id.as_str()),
        case.language,
        &case.source,
        case.cwd.as_deref(),
        AnalysisLimits::new(
            case.source_limit.unwrap_or(GUARD_MAX_SOURCE_BYTES),
            Duration::from_millis(100),
        ),
    )
    .map(|input| input.with_python_write_endpoint(case.python_write_endpoint));
    let mut record = match (input, case.host_profile) {
        (Ok(input), Some(profile)) => {
            let report = analyzer.forecast_edits_with_host_profile(input, profile);
            let mut record = serde_json::to_value(&report)?;
            record["id"] = json!(case.id);
            record["construction"] = json!("accepted");
            if case.include_source_texts {
                record["sourceTexts"] = serde_json::to_value(report.source_texts())?;
            }
            record
        }
        (Ok(input), None) => {
            let report = analyzer.forecast_edits(input);
            let mut record = json!({
                "id": case.id,
                "construction": "accepted",
                "analysis": report,
            });
            if case.include_source_texts {
                record["sourceTexts"] = serde_json::to_value(report.source_texts())?;
            }
            record
        }
        (Err(_), _) => json!({
            "id": case.id,
            "construction": "rejected",
            "analysis": null,
        }),
    };
    if let Some(profile) = case.host_profile {
        record["schema"] = json!("selectiveEditsHostProfileCapture/v1");
        record["hostProfile"] = json!(profile);
    } else {
        record["schema"] = json!("selectiveEditsCapture/v1");
    }
    Ok(record)
}

/// Measure the same semantic boundary as `run`, with transport outside the clock.
pub(crate) fn benchmark() -> Result<(), Box<dyn std::error::Error>> {
    let rounds = benchmark_rounds()?;
    let lines = benchmark_lines()?;
    let mut times: Vec<Vec<u128>> = (0..lines.len())
        .map(|_| Vec::with_capacity(rounds))
        .collect();
    let mut outputs: Vec<Vec<u8>> = vec![Vec::new(); lines.len()];
    let mut result_bytes = 0usize;

    let started = Instant::now();
    let mut analyzer = Analyzer::new()?;
    let construction_ns = started.elapsed().as_nanos();
    for round in 0..=rounds {
        for offset in 0..lines.len() {
            // Bounds above make this identical rotation arithmetic overflow-free.
            let index = (offset + round * 137) % lines.len();
            let started = Instant::now();
            let case: Case = serde_json::from_str(black_box(&lines[index]))?;
            let encoded = serde_json::to_vec(&capture_record(&mut analyzer, &case)?)?;
            black_box(&encoded);
            let elapsed = started.elapsed().as_nanos();

            // Keep the decoded request alive through elapsed, like legacy mode.
            // Semantic checks and timing storage are deliberately outside the clock.
            // A changing fallback/result cannot masquerade as a faster forecast.
            if round == 0 {
                result_bytes = result_bytes.checked_add(encoded.len()).ok_or_else(|| {
                    invalid_input("selective benchmark result-byte count overflow")
                })?;
                if result_bytes > MAX_BENCH_RESULT_BYTES {
                    return Err(
                        invalid_input("selective benchmark result-byte limit exceeded").into(),
                    );
                }
                outputs[index] = encoded;
            } else {
                if encoded != outputs[index] {
                    return Err(invalid_input(format!(
                        "selective benchmark result changed at input {index}, round {round}"
                    ))
                    .into());
                }
                times[index].push(elapsed);
                if round == rounds {
                    outputs[index] = encoded;
                }
            }
        }
    }

    let mut output = std::io::BufWriter::new(std::io::stdout().lock());
    for (index, (times, bytes)) in times.into_iter().zip(outputs).enumerate() {
        let value: serde_json::Value = serde_json::from_slice(&bytes)?;
        serde_json::to_writer(
            &mut output,
            &json!({
                "index": index,
                "constructionNs": construction_ns,
                "elapsedNs": times,
                "result": value,
            }),
        )?;
        writeln!(output)?;
    }
    Ok(())
}

fn benchmark_rounds() -> Result<usize, Box<dyn std::error::Error>> {
    let rounds = match std::env::var("BENCH_ROUNDS") {
        Ok(value) => value.parse::<usize>()?,
        Err(std::env::VarError::NotPresent) => DEFAULT_BENCH_ROUNDS,
        Err(error) => return Err(error.into()),
    };
    if !(1..=MAX_BENCH_ROUNDS).contains(&rounds) {
        return Err(
            invalid_input("BENCH_ROUNDS must be in 1..=100 for --selective-benchmark").into(),
        );
    }
    Ok(rounds)
}

fn benchmark_lines() -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let mut lines = Vec::new();
    let mut input_bytes = 0usize;
    loop {
        let mut line = String::new();
        // Take bounds the allocation even when a physical line has no newline.
        let count = input
            .by_ref()
            .take((MAX_BENCH_LINE_BYTES + 1) as u64)
            .read_line(&mut line)?;
        if count == 0 {
            break;
        }
        if count > MAX_BENCH_LINE_BYTES || lines.len() >= MAX_BENCH_ROWS {
            return Err(invalid_input("selective benchmark line or row limit exceeded").into());
        }
        input_bytes = input_bytes
            .checked_add(count)
            .ok_or_else(|| invalid_input("selective benchmark input-byte count overflow"))?;
        if input_bytes > MAX_BENCH_INPUT_BYTES {
            return Err(invalid_input("selective benchmark input-byte limit exceeded").into());
        }
        // Match BufRead::lines used by ordinary capture, including CRLF handling.
        if line.ends_with('\n') {
            let _ = line.pop();
            if line.ends_with('\r') {
                let _ = line.pop();
            }
        }
        lines.push(line);
    }
    if lines.is_empty() {
        return Err(invalid_input("selective benchmark requires at least one input row").into());
    }
    Ok(lines)
}

fn invalid_input(message: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, message.into())
}
