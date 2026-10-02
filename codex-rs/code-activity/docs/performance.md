# Reproducible performance and stress evidence

The measured unit is a complete source request and its structured activity output.
The consumer pipeline adds the versioned contract envelope, JSON decoding and
discriminator check, typed UI rows, policy classification and consumer JSON
emission. The UI is a tested consumer model; it is not wired into a live Codex
client. Policy output cannot authorize execution, including inputs with no
recognized actions or explicit gaps. Runtime permission enforcement remains the
authoritative boundary.

## Dataset and harness

`tests/fixtures/benchmark-v1.json` is a versioned, synthetic dataset specification.
`tests/support/benchmark.rs` generates its exact inputs, shared by the release
benchmark and stress suite. Prefix/body/suffix repetition yields explicit scale
points; balanced parentheses, the 129-item callback boundary, and exactly
1,048,576/1,048,577 source bytes are generated deterministically. The JSONL export
contains every exact expanded input for independent replay and baseline runs.

Cases cover Python, JavaScript, TypeScript, shell, embedded Node and Python,
nested tool calls, scalar variables, local functions, closures, literal
callbacks, child-process records, recursion, malformed syntax, large comment
inputs, deep syntax, lazy generators, deferred imports, awaited/unawaited promise
content and resource aliases. Inputs are passed exclusively to `Analyzer`; the harness
does not execute them, read their targets, or invoke their process records.
Expected small-case multiplicities are hand-reviewed; capped cases are not
treated as complete effect discovery.

The custom harness avoids adding benchmark dependencies or changing the host
workspace profile. It uses `Instant`, `black_box` on input/output, explicit
warmup, nearest-rank p50/p95, minimum/maximum/mean/population standard deviation,
and every raw microsecond sample. This is descriptive evidence; it provides no
confidence interval, statistical significance test, or production SLA. Parser
initialization, dataset generation, statistics and stdout are outside the timing
window. Request ownership, the consumer's bounded source copy and output
destruction are inside it. AST node counts
are `null`: the public analyzer does not expose traversal instrumentation.

## Exact commands

Run from `codex-rs`, sequentially with two build jobs. Use a quiet machine for
comparative results and record other load. These commands do not imply that a
measurement has run; committed results and the run receipt identify actual runs.

```sh
cargo build -j 2 -p codex-code-activity --release --example benchmark --locked
nice -n 10 target/release/examples/benchmark --emit-corpus > /tmp/benchmark-v1.jsonl
nice -n 10 target/release/examples/benchmark 30 3 all parse-emit > /tmp/benchmark-parse-emit.jsonl
nice -n 10 target/release/examples/benchmark 30 3 all consumer > /tmp/benchmark-consumer.jsonl
cargo test -j 2 -p codex-code-activity --test stress --locked -- --test-threads=1
```

The arguments are measured iterations, warmup iterations, exact case ID (or
`all`), and `parse-emit`/`consumer`. Unknown IDs and invalid iteration counts are
errors. Examples for isolated-process peak memory on macOS:

```sh
/usr/bin/time -l nice -n 10 target/release/examples/benchmark 30 3 python-direct-128 consumer > /tmp/python-direct-128.jsonl 2> /tmp/python-direct-128.time.txt
/usr/bin/time -l nice -n 10 target/release/examples/benchmark 3 1 source-byte-boundary-1048576 consumer > /tmp/source-cap.jsonl 2> /tmp/source-cap.time.txt
```

Peak RSS includes executable/runtime, all generated dataset inputs, parser state,
warmup, output buffers and allocator high-water retention. It is not a per-call
allocation measurement, and must not be described as one. Each selected case runs
in a fresh process; dataset generation is outside timed latency but inside RSS.

## Baselines and interpretation

`parse-emit` measures analysis plus original `Report` JSON and can be compared
with a pinned earlier parser using the exported requests and an equivalent
in-process harness, compiler/profile and iteration scheme. Do not compare it
directly to a baseline that also includes process startup, or call unlike output
counts equivalent. `consumer` has no original-prototype equivalent and measures
the additional requested contract path. Current Codex `ParsedCommand` does not
infer these source meanings, so this document makes no latency comparison to it.

Large action lists can hit operation, binding, function, source, AST traversal or
callback caps: declining output or flat latency then means bounded abstention,
not successful analysis of all actions. Oversized sources reject before parsing;
their throughput is rejected-input throughput. Malformed sources yield explicit
gaps rather than inferred actions. Source caps bound accepted bytes and report
counts; they are not hard wall-clock or native tree-sitter memory limits.

## Actual run receipt

The final current-base measurements cover **111 cases / 222 pipeline runs**, on Apple M1 Ultra,
20 logical CPUs, 64 GiB RAM, macOS 27.0.1 (26A434), native arm64, Rust/Cargo
1.95.0. Measured code commit:
`0b499de7367f947a082bc4560bcf1d5775d44846`. It includes official Codex base
`c5d242fa7907bff1b7a7e26e95febc548c0a6963`. Later receipt/document additions
do not change its compiled parser, tests or benchmark examples; the receipt
hashes every measured Rust file, dataset and driver. The inherited release
profile uses opt-level 3, thin LTO, four codegen units, line-table debug info and
no stripping. Grammar/runtime versions are tree-sitter 0.25.10, Python 0.25.0,
TypeScript 0.23.2 and Bash 0.25.1 (actual locks).

The earlier receipt remains separately in `benchmarks/2026-10-02`, anchored to
`59d2071…` on base `ca466061…`; it is not relabeled as the final build.

Runs were serial at `nice -n 10`, after compilation finished. No Juno release
compiler was observed before the measurements. This was a desktop with ordinary
background applications, without CPU affinity, frequency/thermal control or
statistical isolation. A one-sample consumer pilot selected measured/warmup
counts: mean above 50 ms → 3/1, above 5 ms → 10/2, otherwise 30/3. Counts and
every raw sample are retained; p95 with three samples is simply the maximum.

The complete [run receipt](benchmarks/2026-10-02-current/run-receipt.json),
[raw measurements](benchmarks/2026-10-02-current/measurements.jsonl),
[pilot](benchmarks/2026-10-02-current/pilot-consumer.jsonl) and
[exact compressed source corpus](benchmarks/2026-10-02-current/corpus-v1.jsonl.gz)
are versioned. Expanded corpus SHA-256:
`e3fbada8bf267f2a027e2e38c3a7e4bd2a8f3a95a761033210b215ad27b2dbaf`.
Calls/sec and input bytes/sec use measured iteration count / summed timed
duration; they include ownership, analysis and consumer work, not stdout.

| Consumer case | Input bytes | Median / p95, ms | Ops / gaps / sources | Samples | Calls/sec |
|---|---:|---:|---:|---:|---:|
| Python direct, 1 | 39 | 0.0173 / 0.0206 | 2 / 0 / 1 | 30 | 55,693 |
| Python direct, 16 | 624 | 0.2200 / 0.2308 | 32 / 0 / 1 | 30 | 4,517 |
| Python direct, 128 | 4,992 | 1.7556 / 1.8395 | 256 / 0 / 1 | 30 | 564 |
| Python direct, 512 | 19,968 | 5.6094 / 5.7601 | 256 / 256 / 1 | 10 | 177 |
| JavaScript direct, 128 | 5,144 | 1.0263 / 1.2268 | 128 / 0 / 1 | 30 | 949 |
| Typed TS function, 128 | 1,906 | 0.9557 / 1.0337 | 128 / 0 / 1 | 30 | 1,035 |
| Shell → Node, 32 | 1,984 | 0.5895 / 0.6522 | 32 / 0 / 33 | 30 | 1,674 |
| TS tool → shell → Python, 32 | 3,776 | 1.0750 / 1.1977 | 62 / 1 / 64 | 30 | 920 |
| Python comments, 8,192 | 311,321 | 6,171.4136 / 6,275.4962 | 1 / 0 / 1 | 3 | 0.16 |
| Accepted single-comment source cap | 1,048,576 | 7.7345 / 7.9120 | 0 / 0 / 1 | 10 | 129 |
| Rejected source over cap | 1,048,577 | 0.0287 / 0.0333 | 0 / 1 / 1 | 30 | 34,323 |

These are descriptive host measurements, not performance guarantees. Python
direct-512 hits report caps, and nested tool-32 hits the source cap; their output
is incomplete. Over-cap throughput measures rejection. Empty comment-only
reports are opaque, not evidence of safety. In the separate parse/emission
pipeline, Python direct-1 measured 12.5/19.0 µs median/p95; adding the tested
local consumer measured 17.2/20.6 µs. This is not a live Codex dispatch/event/UI
benchmark and does not include AI inference or permission handling.

## Original parser comparison and memory

The original commit is `125f907ca4ea38ee3c59f20b60d46d9f7e0a4acc`.
A [source-only archive with its Apache-2.0 license](benchmarks/2026-10-02/baseline-125f907.tar.gz)
makes it reproducible without private guidance or discussion files.
[Baseline metadata](benchmarks/2026-10-02-current/baseline-metadata.json) records archive,
identical harness and binary hashes/sizes/profiles. Both use the exact
`examples/compare.rs` source, locked parser dependency versions, Rust 1.95.0,
owned requests, a reused parser, original Report JSON and output destruction,
with 30 measured iterations / three warmups. Setup/input decoding/startup are
outside the timing window.

Of [32 common cases](benchmarks/2026-10-02-current/common-case-ids.json), **14 reports are
byte-identical**. The other 18 retain their timings/counts as semantic deltas,
not equivalent-work speed comparisons. See
[report equivalence hashes](benchmarks/2026-10-02-current/common-equivalence.json),
[summary and variance](benchmarks/2026-10-02-current/comparison-summary.json),
[original raw samples](benchmarks/2026-10-02-current/common-original-125f907.jsonl) and
[current raw samples](benchmarks/2026-10-02-current/common-current.jsonl).

| Byte-identical case | Original median / p95, ms | Current median / p95, ms |
|---|---:|---:|---:|
| Python direct, 128 | 1.3164 / 1.4112 | 1.3500 / 1.4239 |
| JavaScript direct, 128 | 0.7967 / 0.8655 | 0.8041 / 0.9097 |
| Shell → Node, 32 | 0.4513 / 0.5030 | 0.5389 / 0.5561 |
| Awaited JS promises, 128 | 2.0744 / 2.1910 | 2.0603 / 2.1913 |

The current parser generally costs more in these samples; this is a correctness,
resource-control and output-contract change, **not a demonstrated speedup**.
Different current-harness runs vary, particularly for very small scripts; do
not infer statistically significant regressions from this single-host run or
subtract unlike pipelines as exact per-stage costs.

Fresh-process `/usr/bin/time -l` logs cover 12 selected consumer cases.
Their peak RSS ranges from **17,121,280 to 22,560,768 bytes** (16.3–21.5 MiB).
Both common comparison processes use the same 32-input corpus and timed scope:
original peak **7,831,552 bytes**, current **11,845,632 bytes**. Binary sizes are
4,412,664 and 13,707,912 bytes respectively; the current in-tree build also has
Codex dependency/adapter code. These are process footprint observations,
including setup/input buffers/allocator retention and build graph differences,
not per-call allocations or language-only memory regressions. Original/current
common logs and selected `<case>.time.txt` files are in the receipt directory.

## Native parsing limit found by the stress run

The repeated Python comment case grows from 38,937 bytes / 1,024 comment lines
at about 96 ms to 311,321 bytes / 8,192 lines at about **6.17 seconds** in the
consumer run. A separate [parse-only diagnostic](benchmarks/2026-10-02/parse-only.rs),
with the same pinned grammar/runtime and profile, measured native parsing,
root metadata queries and tree destruction at **101 ms / 6.509 seconds**
median. Cursor collection of the already parsed root took **31 µs / 240 µs**.
[Raw diagnostic samples](benchmarks/2026-10-02/parse-only.jsonl) and
[exact diagnostic receipt](benchmarks/2026-10-02/diagnostic-receipt.json) retain
one warmup / three samples, counts and hashes. No submitted source executed.

That diagnostic places the expensive work in the native parse/tree interval,
without pinpointing one native function. The grammar scanner's repeated comment
lookahead is a plausible source-inspection hypothesis, not a profiled attribution.
Neither source bytes nor semantic-visit limits interrupt this interval.
**Cancellation/deadline or native-parser mitigation is required before putting
this path synchronously into an interactive UI.** The prototype has no such
deadline and makes no hard wall-clock or native allocation guarantee.

## Replay the actual runs

From `codex-rs`, extract the source-only baseline, install the identical comparison
harness, and preserve its original manifest/profile:

```sh
mkdir -p /tmp/code-activity-original-125/examples
tar -xzf code-activity/docs/benchmarks/2026-10-02/baseline-125f907.tar.gz -C /tmp/code-activity-original-125
cp code-activity/examples/compare.rs /tmp/code-activity-original-125/examples/compare.rs
CARGO_BUILD_JOBS=2 nice -n 10 cargo build --manifest-path /tmp/code-activity-original-125/Cargo.toml --example compare --release --locked
python3 code-activity/examples/measure.py /tmp/code-activity-original-125/target/release/examples/compare /tmp/code-activity-replay
cp code-activity/docs/benchmarks/2026-10-02/parse-only.rs /tmp/code-activity-original-125/examples/parse-only.rs
CARGO_BUILD_JOBS=2 nice -n 10 cargo build --manifest-path /tmp/code-activity-original-125/Cargo.toml --example parse-only --release --locked
nice -n 10 /tmp/code-activity-original-125/target/release/examples/parse-only > /tmp/code-activity-replay/parse-only.jsonl
```

The actual receipt uses an existing shared local target directory to reuse cached
dependencies; that path change does not alter the compiler/profile. The selected
comparison IDs, expanded corpus and hashes allow replay independently of the
driver. Current runs can be replayed without the baseline. Tests cover repeated
byte-identical reports, count caps, recursion/malformed abstention, small-case
multiplicity and an actual unchanged host sentinel while analyzing write strings.
The sentinel is a regression control, not proof of arbitrary-input safety.
