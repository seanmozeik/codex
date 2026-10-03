# Benchmarking code activity

Run from `codex-rs`. Benchmark inputs are inert source; the harness never executes
their programs or accesses their file/process targets. Save results outside the
repository rather than committing host-specific measurements.

```sh
CARGO_BUILD_JOBS=2 nice -n 10 cargo build -p codex-code-activity --release --example benchmark --locked
nice -n 10 target/release/examples/benchmark 30 3 python-direct-128 parse-emit
nice -n 10 target/release/examples/benchmark 3 1 python-padding-8192 consumer
nice -n 10 target/release/examples/benchmark --emit-corpus > /tmp/code-activity-corpus.jsonl
```

Arguments are measured iterations, warmup iterations, case ID (or `all`) and
`parse-emit`/`consumer`. The versioned dataset in `tests/fixtures/benchmark-v1.json`
and shared generator in `tests/support/benchmark.rs` cover language families,
nested calls, malformed/recursive input and resource boundaries.

`parse-emit` measures request ownership, analysis, Report JSON and output
destruction. `consumer` additionally reviews the paired source/report, emits and
decodes the versioned contract, builds typed UI/policy views and serializes them.
Parser initialization, dataset generation, statistics, stdout and process startup
are excluded. Outputs retain counts and raw durations with descriptive statistics;
p95 with three samples is their maximum. Cap hits and rejected inputs are not
complete analysis throughput. Size/visit limits are not native time guarantees.

For arbitrary JSONL requests, `cargo run --release --example measure -- /tmp/code-activity-corpus.jsonl`
measures complete analysis/serialization. `measure_stream` measures replacement
previews in 256-byte source fragments, excluding final analysis and host I/O.
Compare identical inputs, compiler/profile, output scope and sampling; run builds
and measurements separately. Process RSS includes runtime, input and allocator
retention and is not a per-call allocation metric.
