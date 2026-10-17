# V18 cloud evaluation handoff

This directory contains reviewed authored fixtures, pure saved-data tools and Sean's Rust guidance. No private emails, sessions, traces, credentials, local Git history, native binaries, models or caches are included.

The parent source commit `7fdd2f756dc6b3866d6f1d13656b3058e543c17f` reconstructs exactly V18 tree `bb5be6c2e2fbd17497e0d6b8571c1e1ff8b17427`, exported from local source revision `5e3cd24283b1e3d3c3abfa8892c66ce3e0555b65` over public PR1 `4ce650a1ac114be91904eb9f1919b5c9fcde92ab`. The final branch adds only `jimmy-handoff-v18/`. The original unpublished implementation commit history is not reachable from this branch.

## Start here

Read `rust-skill/SKILL.md`, the relevant reference chapters/rules, and `docs/PROJECT-RUST-STANDARDS.md`. Rust 2024 and pinned 1.95; preserve Codex thin-LTO, workspace lints/dependencies and no-new-features policy. Typed states and lifecycle ownership matter; logical accepted writes, later flush/close, and physical observations must remain distinct. Preserve sandbox/Guardian/approval and original execution behavior.

Source: `codex-rs/code-activity/src/selective_edits/`; strict whole-script API: `codex-rs/code-activity/src/codex_whole_script.rs`; core consumer: `codex-rs/core/src/tools/code_mode/source_description/`; examples and regression tests under `codex-rs/code-activity/`. V18-specific regressions are `selective_python_endpoints`, `selective_python_suffix_touches`, snapshot endpoint tests and core/TUI `known_later_touch_` tests.

The original strict whole-script API and conditional selective UI lane are separate. Syntax acceptance is not effect correctness. Retain understood edits only with defensible scope/state/dependency evidence; preserve barriers and assumptions for unsupported work.

## Saved evidence and limits

V18 repaired 3 legacy source-contract failures to 0. 361 selected native tests passed (358 activity/example, 1 core, 2 TUI); scoped Clippy passed. Full workspace CI/runtime-oracle/visual acceptance is not claimed.

Credited development coverage remained Old88 40/58 operations and Former24 8/33; Fresh40 fixed/expanded lanes were 4/27,8/27,10/27. New Python bindings represent 4 conditional logical projections plus 1 successful-open truncation out of 14 original Python primitives; Node 10/13. Do not relabel mixed endpoints as observed file edits. Over-80% broad coverage and fresh model accuracy have not been established.

The supplied 24 Python contract cases and 40 Fresh40 Python/Node cases are **exposed authored development fixtures**, not natural sessions or independent holdouts. Keep the cloud owner's separately frozen challenges separate. Expected V18 response files are saved behavior, not independent gold; only timing fields were removed.

Python24 includes independent authored byte/proof gold and an adapter to unchanged semantic-checker functions. The new adapter uses a package-specific pin manifest and a fixed 24-case denominator; it does not invoke the old historical CLI. Saved V18 replay had zero case errors; corrupt-byte and missing-row controls were rejected. Fresh40 includes original authored cases/gold but only a saved-behavior comparator here: a full portable semantic scorer must be reviewed before making new Fresh40 correctness/coverage claims. Retain all 27 original primitives and all negatives. Matching behavior is a regression check, not an accuracy proof.

No new V18 runtime p50/p95/p99/RSS measurement was made for the handoff. Benchmark release builds on the identical corpus, preserve actual binary/source identities and process receipts, distinguish in-process timing from wall/RSS, and never use build RSS as parser-runtime RSS. `docs/NATIVE-COMMANDS.json` lists the earlier scoped Cargo commands with host paths removed.

## Reproduce in the cloud

From `codex-rs`, after the environment's approved dependency setup:

```sh
cargo build --locked --jobs 1 --release -p codex-code-activity --example source_boundary --example snapshot_preview_boundary
target/release/examples/snapshot_preview_boundary < ../jimmy-handoff-v18/fixtures/python24/requests.jsonl > /new/output/python24.jsonl
target/release/examples/snapshot_preview_boundary < ../jimmy-handoff-v18/fixtures/fresh40/requests.jsonl > /new/output/fresh40.jsonl
python3 -I -B ../jimmy-handoff-v18/tools/score_python24.py --responses /new/output/python24.jsonl --candidate-label YOUR_COMMIT --output /new/output/python24-score.json
python3 -I -B ../jimmy-handoff-v18/tools/compare_saved.py --expected ../jimmy-handoff-v18/fixtures/fresh40/expected-v18-results.jsonl --responses /new/output/fresh40.jsonl
```

Use new output paths and adjust for any `CARGO_TARGET_DIR`. The comparator returns 1 for every changed result, including improvements; judge changes against independent gold. The boundary reads source and supplied bytes as inert data, and does not open fixture target paths or execute represented commands. `BENCH_ROUNDS=31` with `--benchmark` measures repeated deterministic boundary behavior; it is not a population sample. Empty caches cannot build offline; do not transfer Studio caches.

Likely next structural gaps from prior diagnosis: nested contexts, aliases/owner lifetime, ordered multiple writes, append/update cursor and tell/seek cookies, and over-broad whole-source rejection. Confirm current barriers before tuning. Preserve Unicode character counts versus byte offsets, newline policy, stream revision versus filesystem epoch, lazy control flow and completion conditions.
