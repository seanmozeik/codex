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

The integration owner records the exact code/dataset hashes, machine and OS,
compiler and Cargo versions, release profile, load conditions, commands, measured
sample counts, raw JSONL and isolated RSS logs here after execution. No measured
numbers are claimed until those artifacts exist. Tests cover deterministic
byte-identical reports, bounded records/gaps/source counts, explicit recursive
and malformed abstention, small-case action multiplicity, and an actual unchanged
host sentinel when file-writing strings are analyzed. The sentinel test is a
regression control, not a proof of arbitrary-input safety.
