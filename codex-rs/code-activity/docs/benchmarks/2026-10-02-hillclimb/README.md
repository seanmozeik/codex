# Scanner hill-climb evidence — 2 October 2026

Measured source anchor: `3b037e19e538cf8ec20b1463c4fa2bb81e564b72`.
Official Codex base: `c5d242fa7907bff1b7a7e26e95febc548c0a6963`.
The earlier published build is `e20092b830d80acb90d637e4480f723450eef042`,
whose compiled Rust anchor is `0b499de7367f947a082bc4560bcf1d5775d44846`.
Later documentation/receipt commits do not change compiled Rust or scanner bytes.

Analysis and consumers are Rust. Tree-sitter runtime and generated grammars are
native C through Rust bindings; no Tripwire TypeScript implementation or JS
runtime is added. The MIT Python grammar package is vendored for the small
scanner guard, with unchanged generated parser, original bindings/build script,
license, 17-file source manifest and exact scanner patch. Cargo and Bazel use
that same checked-in scanner. `grammar.js` is not evaluated by the analyzer.

All workload programs remain inert. Drivers invoke only compiled analyzer/C
probe executables; they do not execute Python/Node/shell fixtures or their
process/file targets. Ordinary desktop host measurements have no thermal,
frequency or statistical isolation and establish no production latency bound.

## Artifacts and timed scopes

- [Full clean pipeline receipt](pipeline/run-receipt.json),
  [222 raw measurements](pipeline/measurements.jsonl),
  [pilot](pipeline/pilot-consumer.jsonl),
  [exact 111-case corpus](pipeline/corpus-v1.jsonl.gz),
  [32 original/current comparisons](pipeline/comparison-summary.json).
  The unchanged existing `examples/measure.py` generates these; 12 isolated RSS
  logs and both baseline comparison logs accompany them. Sampling is explicitly
  selected by pilot thresholds. Original-prototype semantic deltas are not
  equivalent-work speed comparisons.
- [Matched Rust receipt](matched/rust-build-and-replay-receipt.json),
  [16 matched consumer measurements](matched/matched-consumer.jsonl),
  [111 exact Report comparisons](matched/report-equivalence.json),
  [before reports](matched/reports-before.jsonl.gz) and
  [after reports](matched/reports-after.jsonl.gz). Same counts/warmup/timed scopes
  per variant; padding 3/1, other selected cases 30/3. Reports include every
  original span/uncertainty record. Size/count equality alone is not this gate.
  RSS includes startup, generated corpus, runtime and allocator retention.
- [Native build receipt](native/native-build-receipt.json),
  [24 native scaling rows](native/native-scaling.jsonl),
  [12 paired scaling summaries](native/native-scaling-summary.json),
  [native reproduction/scope](native/README.md),
  [213 exact full-CST results and 639 incremental checks](native/native-equivalence-results.json).
  Incremental checks split into 526 changed-input edits and 113 no-ops.
  Native timing includes reused-parser parsing and Tree destruction; full
  inspection occurs afterward. It excludes Rust interpretation/consumers.
  FNV signatures in timed rows are diagnostic; correctness compares complete
  CST byte sequences, including comments/anonymous nodes and all span/flag data.
- [Before profile call graph excerpt](native/profile-before.callgraph.txt),
  [profile-active samples](native/profile-before.jsonl),
  [profile command/binary hash](native/profile-receipt.json).
  The excerpt excludes the binary-image inventory. Profile-active timings are
  excluded from clean matched/pipeline claims. The profile and native source
  inspection identify repeated external-scanner comment suffix lookahead.
- [Current gate log](verification.log): 147 tests with zero retries/skips,
  four doctests, strict scoped Clippy/fmt/rustdoc/release, 20 native Bazel test
  targets and 18 skipped Windows cross targets. See [verification](../../verification.md)
  for existing upstream/missing-tool limitations; skipped cross targets do not
  validate Windows.

The module-comment native scaling family becomes approximately linear; the
low-indented-comment family intentionally keeps original lookahead and remains
approximately quadratic. That negative control is retained explicitly. No
source stripping, smaller output caps or loss of uncertainty is used. Native
cancellation/deadline work remains necessary for arbitrary-source UI bounds.

## Replay Rust measurements

From the fork root, build the repaired candidate and a clean pinned before
checkout serially, with two low-priority jobs. These commands are reproduction
instructions; the receipts above identify the actual observed runs.

```sh
git worktree add --detach /tmp/code-activity-before e20092b830d80acb90d637e4480f723450eef042
CARGO_BUILD_JOBS=2 nice -n 10 cargo build --manifest-path /tmp/code-activity-before/codex-rs/Cargo.toml --release --bin codex-code-activity --example benchmark --locked
CARGO_BUILD_JOBS=2 nice -n 10 cargo build --manifest-path codex-rs/Cargo.toml --release --bin codex-code-activity --example benchmark --example compare --locked
cd codex-rs
python3 code-activity/docs/benchmarks/2026-10-02-hillclimb/replay-rust.py /tmp/code-activity-before/codex-rs/target/release/examples/benchmark /tmp/code-activity-before/codex-rs/target/release/codex-code-activity /tmp/hillclimb-matched --rebuilt-before /tmp/code-activity-before
```

Build paths/compiler differences may change binary hashes. The optional
`--rebuilt-before` validates a clean pinned Git checkout and records that replay
mode; the actual committed run used the preserved before binary matching its
published receipt. Every actual supplied binary hash remains recorded.

The complete pipeline uses `examples/measure.py`; [performance reproduction](../../performance.md#replay-the-actual-runs)
shows how to build the source-only original 125f907 comparison harness. Supply
that original `compare` binary and a new output directory, then run serially:

```sh
python3 code-activity/examples/measure.py /tmp/code-activity-original-125/target/release/examples/compare /tmp/hillclimb-pipeline
```

[Native instructions](native/README.md) reproduce both C builds, exact fresh and
incremental CST checks, comment scaling and the retained-path negative controls.
