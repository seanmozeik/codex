//! Measure incremental preview updates over a local corpus.

use codex_code_activity::ActivityStream;
use codex_code_activity::Request;
use std::hint::black_box;
use std::io::BufRead;
use std::io::Write;
use std::time::Duration;
use std::time::Instant;

fn main() -> Result<(), eyre::Report> {
    let path = std::env::args()
        .nth(1)
        .ok_or_else(|| eyre::eyre!("usage: measure_stream corpus.jsonl"))?;
    let input = std::io::BufReader::new(fs_err::File::open(path)?);
    let mut times = vec![];
    let mut cells = 0;
    let mut json_bytes = 0;
    let mut source_bytes = 0;
    for line in input.lines() {
        let request: Request = serde_json::from_str(&line?)?;
        source_bytes += request.source.len();
        let mut stream = ActivityStream::new(request.call_id, request.language, request.cwd)?;
        let mut start = 0;
        while start < request.source.len() {
            let mut end = (start + 256).min(request.source.len());
            while !request.source.is_char_boundary(end) {
                end -= 1;
            }
            let now = Instant::now();
            let update = stream.append(&request.source[start..end])?;
            let json = serde_json::to_vec(&update)?;
            times.push(now.elapsed());
            json_bytes += json.len();
            black_box(json);
            start = end;
        }
        let _ = black_box(stream.finish()?);
        cells += 1;
    }
    if times.is_empty() {
        eyre::bail!("empty corpus");
    }
    times.sort_unstable();
    let percentile = |n: usize| times[(times.len() - 1) * n / 100].as_secs_f64() * 1e6;
    let result = serde_json::json!({
        "cells": cells, "updates": times.len(), "sourceBytes": source_bytes, "jsonBytes": json_bytes,
        "chunkBytes": 256,
        "microsecondsPerUpdate": {"p50": percentile(50), "p95": percentile(95), "p99": percentile(99), "max": percentile(100)},
        "totalMilliseconds": times.iter().sum::<Duration>().as_secs_f64() * 1000.0,
        "scope": "incremental parse, prefix semantic analysis, and replacement-view JSON per 256-byte fragment; excludes input decoding, initial parser setup, final analysis, filesystem and process I/O"
    });
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, &result)?;
    writeln!(stdout)?;
    Ok(())
}
