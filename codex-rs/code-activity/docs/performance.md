# Reproducible performance and stress evidence

The measured unit is a complete source request and its structured activity output.
The consumer pipeline adds the versioned contract envelope, JSON decoding and
discriminator check, typed UI rows, policy classification and consumer JSON
emission. The UI is a tested consumer model; it is not wired into a live Codex
client. Policy output cannot authorize execution, including inputs with no
recognized actions or explicit gaps. Runtime permission enforcement remains the
authoritative boundary.

## Scanner hill climb: current repaired measurements

The 311,321-byte / 8,192-comment Python case now takes **6.262 ms median**
through the full Rust consumer, versus **5,987.412 ms** in a fresh matched run
of the previously published build: about **956× faster** on this fixture.
At 1,024 comments the matched median falls from 94.518 ms to 0.793 ms.
All **111 serialized Report outputs are byte-identical** before/after,
including operations, uncertainty records, source spans and nested provenance.

Activity interpretation, the Codex adapter and consumers are Rust. Syntax parsing
uses tree-sitter's native C runtime and generated C grammars through Rust FFI.
There is no vendored or executed Tripwire TypeScript implementation and no
JavaScript runtime in the analyzer path. This repair vendors the MIT Python
0.25.0 grammar package under `third_party/tree-sitter-python`. Its 16 unchanged
selected package files include the generated C parser and Rust bindings/build
script. Only the native scanner changes. `grammar.js` remains provenance
and generator input; Cargo does not evaluate it. The source manifest and exact
patch are retained beside the grammar. Cargo and Bazel both compile that scanner.

The repaired measured source anchor is
`3b037e19e538cf8ec20b1463c4fa2bb81e564b72`, on the same official c5d242fa base,
compiler and inherited release profile as the pre-repair build. The preserved
before benchmark binary hash matches its old run receipt. Its compiled source
anchor is 0b499de7367f947a082bc4560bcf1d5775d44846, published as e20092b830d80acb90d637e4480f723450eef042;
later pre-repair edits were docs/data only. Current receipts include binaries,
Rust/data/driver hashes, grammar sources, Cargo/Bazel locks and exact commands.

| Matched consumer case | Before median / p95, ms | After median / p95, ms | Samples each |
|---|---:|---:|---:|
| python-padding-1024 | 94.5180 / 95.6248 | 0.7934 / 0.7935 | 3 |
| python-padding-8192 | 5987.4123 / 6003.8470 | 6.2615 / 6.4805 | 3 |
| python-direct-1 | 0.0173 / 0.0212 | 0.0170 / 0.0213 | 30 |
| python-direct-128 | 1.7512 / 1.8708 | 1.7554 / 1.8368 | 30 |
| javascript-direct-128 | 1.0175 / 1.1071 | 1.0210 / 1.0935 | 30 |
| typescript-functions-128 | 0.9553 / 1.0002 | 0.9546 / 1.0281 | 30 |
| shell-direct-128 | 1.1965 / 1.3237 | 1.1918 / 1.3171 | 30 |
| shell-nested-node-32 | 0.5869 / 0.6539 | 0.5877 / 0.6851 | 30 |

The padding comparisons use three measured calls and one warmup per variant;
other matched cases use 30/3. Every raw duration, min/max/mean and population
standard deviation is retained. p95 for three samples is their maximum.
Commands run serially at nice 10 without profiling or builds; no active compiler
was observed before measurement. The ordinary desktop/background-load caveats
below apply. Regular-script medians stayed close in this sample. Individual median/ p95
changes and raw variance remain visible rather than being summarized as an
across-the-board win. This is not a statistical non-regression proof or a
universal speedup claim.

The separate refreshed 111-case driver retains 222 parse/emission and consumer
measurements, 12 isolated RSS logs and 32 original-prototype comparisons.
It measured the repaired 8,192-comment consumer at 6.410/6.599 ms median/ p95
(10 samples), and the tiny Python consumer at 17.0/20.7 µs (30 samples).
Sample counts differ from the matched run and are not pooled. The expanded
corpus hash remains e3fbada8bf267f2a027e2e38c3a7e4bd2a8f3a95a761033210b215ad27b2dbaf.

### Profile, native scaling and preserved syntax

The retained three-second/one-millisecond sampled call graph places 2019 of 2159
samples beneath Python's external scanner and 1430 top-of-stack samples in
`ts_lexer__do_advance`. The scanner rescanned successive remaining comment
suffixes, then returned false; the ordinary lexer reset and consumed each
original comment. Profile-active timings are retained only as diagnostic data
and are excluded from the clean comparison above.

The guard records the **first** comment's indentation, then returns false only
when INDENT/NEWLINE are disabled and that indentation rules out DEDENT.
A comment already rules out STRING_START; recovery requires INDENT and keeps
its original path. No source preprocessing, comment removal, syntax validation
skip, output cap reduction or permission fallback is used. Review includes every
subsequent scanner return and the runtime's lexer reset on false.

The separate serial clang-O3 native diagnostic (same C driver/flags, parser
reused, parse plus Tree destruction, inspection outside timing) records:

| Native module comments | Before median, ms | After median, ms |
|---|---:|---:|
|128|2.284|0.118|
|512|23.578|0.341|
|1,024|93.811|0.679|
|2,048|373.767|1.382|
|4,096|1,504.630|2.845|
|8,192|5,999.615|5.340|

Same-indentation function comments improve similarly: 4,096 comments fall from
1,294.968 to 2.349 ms. These are native diagnostics, not Rust consumer numbers.
Exact complete CST comparison passes 213 fresh sources (117 from the pinned
upstream grammar corpus plus 96 generated), and 639 incremental checks:
526 source-changing edits plus 113 no-op indentation edits. Comparisons include
all children/comments, kinds/fields, byte/point spans and named/extra/error/missing
flags; candidate/upstream and incremental/fresh trees match. Timed FNV signatures
are diagnostic only. Eight new Rust tests additionally cover syntax ownership,
ordering/spans, Unicode/tabs/CRLF/strings, malformed abstention, nested source/UI
provenance and a late sensitive write blocked before mock dispatch.

**Remaining native time limit:** lower-indented comments can still require
lookahead to decide DEDENT. The explicit unchanged-path negative control is
68.925→68.796 ms at 1,024 comments and 272.019→273.873 ms at 2,048 comments.
That family remains approximately quadratic. Native parsing still has no hard
cancellation/deadline or allocation limit, so this fix does not establish a
safe synchronous UI latency bound for arbitrary input.

[Hill-climb reproduction and artifact index](benchmarks/2026-10-02-hillclimb/README.md)
links the clean matched samples/RSS/report hashes, current complete pipeline,
profile excerpt, native build receipt, raw scaling and exact-CST corpus/results.

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
warmup, nearest-rank p50/ p95, minimum/maximum/mean/population standard deviation,
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

## Pre-optimization run receipt

The preserved pre-optimization measurements cover **111 cases / 222 pipeline runs**, on Apple M1 Ultra,
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
`59d2071…` on base `ca466061…`; it is not relabeled as the repaired build.

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
pipeline, Python direct-1 measured 12.5/19.0 µs median/ p95; adding the tested
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

Before the scanner repair, the in-tree parser generally cost more than the
original prototype in these samples. Those correctness/resource/contract
changes did not demonstrate a general speedup. The targeted scanner gains above
are compared with the pre-repair in-tree build.
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

## Pre-optimization native parsing limit

The repeated Python comment case grows from 38,937 bytes / 1,024 comment lines
at about 96 ms to 311,321 bytes / 8,192 lines at about **6.17 seconds** in the
consumer run. A separate [parse-only diagnostic](benchmarks/2026-10-02/parse-only.rs),
with the same pinned grammar/runtime and profile, measured native parsing,
root metadata queries and tree destruction at **101 ms / 6.509 seconds**
median. Cursor collection of the already parsed root took **31 µs / 240 µs**.
[Raw diagnostic samples](benchmarks/2026-10-02/parse-only.jsonl) and
[exact diagnostic receipt](benchmarks/2026-10-02/diagnostic-receipt.json) retain
one warmup / three samples, counts and hashes. No submitted source executed.

The old diagnostic isolated native parsing but did not profile individual
functions. The new sampled call graph and scanner repair above substantiate the
repeated-comment lookahead attribution. Neither source-byte nor semantic-visit
limits interrupt unchanged expensive native paths. Cancellation/deadline work
remains required before relying on bounded synchronous UI latency.

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
