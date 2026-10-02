//! Measure complete-call analysis and JSON serialization.

use codex_code_activity::Analyzer;
use codex_code_activity::Request;
use std::hint::black_box;
use std::io::BufRead;
use std::io::Write;
use std::time::Duration;
use std::time::Instant;

fn main() -> Result<(), eyre::Report> {
    let path = std::env::args()
        .nth(1)
        .ok_or_else(|| eyre::eyre!("usage: measure corpus.jsonl"))?;
    let input = std::io::BufReader::new(fs_err::File::open(path)?);
    let mut analyzer = Analyzer::new()?;
    let mut times = vec![];
    let mut bytes = 0;
    let mut operations = 0;
    let mut with_operations = 0;
    let mut json_bytes = 0;
    for line in input.lines() {
        let request: Request = serde_json::from_str(&line?)?;
        bytes += request.source.len();
        let started = Instant::now();
        let report = analyzer.analyze(request);
        let json = serde_json::to_vec(&report)?;
        times.push(started.elapsed());
        operations += report.operations.len();
        with_operations += usize::from(!report.operations.is_empty());
        json_bytes += json.len();
        black_box(json);
    }
    if times.is_empty() {
        eyre::bail!("empty corpus");
    }
    times.sort_unstable();
    let percentile = |n: usize| times[(times.len() - 1) * n / 100].as_secs_f64() * 1e6;
    let result = serde_json::json!({"samples":times.len(),"sourceBytes":bytes,"jsonBytes":json_bytes,"operations":operations,"samplesWithOperations":with_operations,"microseconds":{"p50":percentile(50),"p95":percentile(95),"p99":percentile(99),"max":percentile(100)},"totalMilliseconds":times.iter().sum::<Duration>().as_secs_f64() * 1000.0,"scope":"analysis plus JSON serialization; one reused analyzer; excludes input JSON decoding and process startup"});
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, &result)?;
    writeln!(stdout)?;
    Ok(())
}
