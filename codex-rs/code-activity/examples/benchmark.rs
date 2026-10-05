//! Reproducible release benchmark; all source programs remain inert.

#[path = "../tests/support/benchmark.rs"]
mod dataset;

use codex_code_activity::Analyzer;
use codex_code_activity::Report;
use codex_code_activity::contract::ActivityContract;
use codex_code_activity::policy::review_script;
use std::hint::black_box;
use std::io::Write;
use std::time::Duration;
use std::time::Instant;

#[derive(Clone, Copy, Debug)]
enum Pipeline {
    ParseEmit,
    Consumer,
}

#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct OutputCounts {
    operations: usize,
    unresolved: usize,
    sources: usize,
    output_bytes: usize,
    consumer_bytes: usize,
}

fn pipeline(
    analyzer: &mut Analyzer,
    case: &dataset::Case,
    mode: Pipeline,
) -> anyhow::Result<OutputCounts> {
    match mode {
        Pipeline::ParseEmit => {
            let report = analyzer.analyze(black_box(case.request()));
            let json = serde_json::to_vec(&report)?;
            Ok(output_counts(&report, &json, 0))
        }
        Pipeline::Consumer => {
            let reviewed = review_script(analyzer, black_box(case.request()));
            let report = reviewed.report();
            let contract = ActivityContract::from_report(report);
            let json = serde_json::to_vec(&contract)?;
            let decoded: serde_json::Value = serde_json::from_slice(black_box(&json))?;
            anyhow::ensure!(
                decoded["schema"] == "codex.codeActivity.v1",
                "unexpected contract schema"
            );
            let rows = contract.ui_rows();
            let consumers = serde_json::to_vec(&(rows, reviewed.decision()))?;
            let bytes = consumers.len();
            black_box(consumers);
            Ok(output_counts(report, &json, bytes))
        }
    }
}

const fn output_counts(report: &Report, json: &[u8], consumer_bytes: usize) -> OutputCounts {
    let result = OutputCounts {
        operations: report.operations.len(),
        unresolved: report.unresolved.len(),
        sources: report.sources.len(),
        output_bytes: json.len(),
        consumer_bytes,
    };
    black_box(json);
    result
}

fn statistics(samples: &[Duration]) -> anyhow::Result<serde_json::Value> {
    let count = f64::from(u32::try_from(samples.len())?);
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let raw: Vec<_> = samples
        .iter()
        .map(|sample| sample.as_secs_f64() * 1e6)
        .collect();
    let mean = raw.iter().sum::<f64>() / count;
    let variance = raw
        .iter()
        .map(|sample| (sample - mean).powi(2))
        .sum::<f64>()
        / count;
    let percentile = |percent: usize| {
        sorted[(sorted.len() * percent).div_ceil(100).saturating_sub(1)].as_secs_f64() * 1e6
    };
    Ok(serde_json::json!({
        "raw": raw, "min": sorted[0].as_secs_f64() * 1e6,
        "p50": percentile(50), "p95": percentile(95), "max": percentile(100),
        "mean": mean, "populationStddev": variance.sqrt(),
        "totalSeconds": samples.iter().sum::<Duration>().as_secs_f64(),
    }))
}

fn emit_corpus(cases: &[dataset::Case]) -> anyhow::Result<()> {
    let mut stdout = std::io::stdout().lock();
    for case in cases {
        serde_json::to_writer(
            &mut stdout,
            &serde_json::json!({
                "callId": case.id, "language": case.language,
                "source": case.source, "cwd": null, "snapshots": [],
            }),
        )?;
        writeln!(stdout)?;
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let cases = dataset::cases()?;
    if args.first().is_some_and(|arg| arg == "--emit-corpus") {
        return emit_corpus(&cases);
    }
    let iterations: usize = args.first().map_or(Ok(30), |arg| arg.parse())?;
    let warmup: usize = args.get(1).map_or(Ok(3), |arg| arg.parse())?;
    let selected = args.get(2).map_or("all", String::as_str);
    let mode = match args.get(3).map_or("consumer", String::as_str) {
        "parse-emit" => Pipeline::ParseEmit,
        "consumer" => Pipeline::Consumer,
        other => anyhow::bail!("unknown pipeline {other}; use parse-emit or consumer"),
    };
    anyhow::ensure!(
        (1..=10_000).contains(&iterations),
        "iterations must be 1..=10000"
    );
    anyhow::ensure!(warmup <= 1000, "warmup must be <=1000");
    let mut analyzer = Analyzer::new()?;
    let mut stdout = std::io::stdout().lock();
    let mut matched = false;
    for case in cases
        .iter()
        .filter(|case| selected == "all" || case.id == selected)
    {
        matched = true;
        for _ in 0..warmup {
            black_box(pipeline(&mut analyzer, case, mode)?);
        }
        let mut samples = Vec::with_capacity(iterations);
        let mut output = OutputCounts::default();
        for _ in 0..iterations {
            let started = Instant::now();
            output = black_box(pipeline(&mut analyzer, case, mode)?);
            samples.push(started.elapsed());
        }
        let timings = statistics(&samples)?;
        let total = samples.iter().sum::<Duration>().as_secs_f64();
        let count = f64::from(u32::try_from(iterations)?);
        let bytes = f64::from(u32::try_from(case.source.len())?);
        serde_json::to_writer(
            &mut stdout,
            &serde_json::json!({
                "benchmarkVersion": 1, "datasetVersion": 1, "case": case.id,
                "language": case.language, "scale": case.scale, "inputBytes": case.source.len(),
                "effectsPerUnit": case.effects_per_unit, "requiredGap": case.required_gap,
                "iterations": iterations, "warmup": warmup, "pipeline": format!("{mode:?}"),
                "microseconds": timings, "callsPerSecond": count / total,
                "inputBytesPerSecond": bytes * count / total, "output": output,
                "astNodes": null,
                "scope": "request ownership; reused parser analysis; JSON emission; consumer mode additionally reviews paired source/report, decodes JSON, validates discriminator, builds typed UI/policy views and emits them; excludes setup, input generation, stdout and process startup",
            }),
        )?;
        writeln!(stdout)?;
    }
    anyhow::ensure!(matched, "unknown benchmark case {selected}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::statistics;
    use std::time::Duration;

    #[test]
    fn descriptive_statistics_preserve_samples_and_use_nearest_rank() -> anyhow::Result<()> {
        let samples = [4, 1, 3, 2].map(Duration::from_micros);
        let stats = statistics(&samples)?;
        let number = |name: &str| -> anyhow::Result<f64> {
            stats[name]
                .as_f64()
                .ok_or_else(|| anyhow::anyhow!("missing statistic {name}"))
        };
        assert_eq!(stats["raw"], serde_json::json!([4.0, 1.0, 3.0, 2.0]));
        for (name, expected) in [
            ("min", 1.0),
            ("p50", 2.0),
            ("p95", 4.0),
            ("max", 4.0),
            ("mean", 2.5),
        ] {
            assert!((number(name)? - expected).abs() < 1e-9, "{name}: {stats}");
        }
        assert!((number("populationStddev")? - 1.25_f64.sqrt()).abs() < 1e-9);
        Ok(())
    }
}
