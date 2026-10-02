# Verification — 2 October 2026

Environment: macOS arm64, Rust 1.95.0, cargo-nextest 0.9.146, Python 3.14.7,
Node 26.10.0, Bazelisk 1.29.0 selecting Codex's pinned Bazel 9.0.0.
Current verified and merged official Codex base: `c5d242fa7907bff1b7a7e26e95febc548c0a6963`.
Actual Tripwire comparison: `56f18dad3ac729db75e6d2437fa74b1b9a3c900f`.

| Check | Actual current result |
|---|---|
| In-tree nextest, all targets including ignored fixed runtime fixtures | **147 passed, zero skipped**, zero failures/retries in final run; 2.370 seconds with two test threads |
| In-tree lifecycle/API doctests | **4 passed**, including three compile-fail lifecycle/ID checks |
| Changed-crate all-target Clippy, warnings denied plus pedantic/nursery | Passed; no suppressions or weakened lints |
| Formatting and warning-denied rustdoc | Passed; stable rustfmt retains the unchanged upstream nightly-setting warning |
| Current-base release library, CLI and actual adapter/consumer/benchmark examples | Passed; inherited thin-LTO profile; two low-priority build jobs |
| Earlier-base all-target release build | Passed on `ca466061…`; preserved in the earlier receipt, not relabeled as current-base release validation |
| Native Bazel `//codex-rs/code-activity:all` | **20 targets passed; 18 Windows cross targets skipped** on macOS |
| Bazel module-lock error mode | Passed after regenerating registry-to-local Python grammar metadata; no unrelated versions changed |
| Bazel/Clippy policy consistency script | Passed |
| Workspace manifest policy script | Existing unchanged upstream failure: stale code-mode feature exception; earlier reproduced in pristine base |
| Source-size check | All **65 authored Rust files** are at most 350 lines: 64 crate/example/test files plus the separate 115-line diagnostic; the two unmodified upstream Rust binding/build files also fit that limit |
| Stress and timed evidence | **111 inert cases / 222 pipeline measurements**, 32 original/current common comparisons, 16 matched before/after consumer runs, raw durations/RSS, 24 native scaling runs including retained-path controls |
| Standalone diagnostic compilation and strict Clippy | Passed against the original locked grammar/profile; no source execution |
| Measured source/data integrity | Measured Rust/data source anchor `3b037e19…`; exact corpus remains unchanged; before benchmark hash matches published receipt; native grammar/locks/binaries have supplemental hashes |
| Native syntax equivalence | **213 exact full-CST comparisons / 639 incremental checks**: 526 changed-input edits plus 113 no-ops; candidate/upstream and incremental/fresh match |
| Full Report equivalence across scanner repair | **111/111 byte-identical** reports, including operation order, spans and uncertainty |
| Codex compatibility | Real item adapter tests pass; no changes to core, app-server, protocols or TUI relative to official base |
| Original checkouts | Parser `125f907…` and Tripwire `56f18da…` remain clean and untouched |
| Frozen older Tripwire corpus | Byte-for-byte preserved; 191 full operation comparisons remain exact; a generator uncertainty exception is narrow and explicit |
| Public content/provenance | Synthetic inputs only; MIT Tripwire attribution retained; source-only original baseline retains its Apache-2.0 license |

The current 147 tests include the original regression suite, 12 real Codex item
boundary tests, contract/policy consumers, semantic/resource repairs and scaling
stress, plus eight new native-comment/order/coordinate/contract/security tests. Test count is not fixture count: one function-golden test iterates 57
ordered-effect programs, and the historical corpus contains 355 commands / 273
embedded inputs. New goldens retain ordering and provenance; the historical set
comparison deduplicates identical operations. The old standalone 88-test result
belongs to an earlier snapshot and is not reused as current in-tree validation.

The two fixed runtime fixtures run Python and Node in fresh temporary directories.
All adversarial/generated-source probes remain inert strings. Bazel's default
Rust harness does not run the two ignored runtime fixtures; Cargo's explicit
`--run-ignored all` run above does. Skipped Bazel cross targets are not Windows
validation. No Linux/remote-executor or full upstream CI result is claimed.

## Reproduce in the fork

From `codex-rs`:

```sh
cargo fmt -p codex-code-activity --check
cargo check -p codex-code-activity --all-targets --locked
cargo clippy -p codex-code-activity --all-targets --all-features --no-deps --locked -- -D warnings -W clippy::pedantic -W clippy::nursery
CARGO_BUILD_JOBS=2 nice -n 10 cargo nextest run --test-threads 2 -p codex-code-activity --all-targets --all-features --locked --no-fail-fast --run-ignored all --retries 0
cargo test -p codex-code-activity --doc --locked
RUSTDOCFLAGS='-D warnings' cargo doc -p codex-code-activity --no-deps --locked
CARGO_BUILD_JOBS=2 nice -n 10 cargo build -p codex-code-activity --release --lib --bin codex-code-activity --example benchmark --example compare --example consumer --example codex_item --locked
cargo run -p codex-code-activity --example codex_item --locked
```

`--no-deps` scopes the additional pedantic/nursery lint groups to this changed
crate rather than imposing new policy on unchanged upstream path dependencies.
The crate inherits Codex's existing dependency/lint configuration and explicitly
forbids unsafe Rust. The retained upstream grammar bindings use their original
FFI/build code; they are not newly authored safe Rust. The separate standalone gate retains its stricter original
configuration; no local Clippy config shadows Codex's root config here.
Stable rustfmt reports the upstream nightly-only imports_granularity setting;
imports are already separate items.

From the repository root, with its pinned Bazel available:

```sh
bazel mod deps --lockfile_mode=error
bazel test //codex-rs/code-activity:all --jobs=2 --test_output=errors
python3 .github/scripts/verify_bazel_clippy_lints.py
python3 .github/scripts/verify_cargo_workspace_manifests.py
```

The last command fails on unchanged `codex-rs/code-mode/Cargo.toml` at this base,
not on the new crate. No unrelated manifest or checker exception was changed.
Bazel dependency-version warnings for platforms/rules_cc also predate this work.

`cargo-deny` and `cargo-shear` are not installed here, so their automated gates
were not run. New registry-package license metadata was checked directly:
fs-err is MIT OR Apache-2.0; both grammar crates are MIT. These licenses are
allowed by upstream deny.toml. This manual check is not a full advisory/deny run.

[Current scanner-repair gate log](benchmarks/2026-10-02-hillclimb/verification.log)
records the new 147-test/native build results. The earlier pre-repair
[verification log](verification.log) remains preserved with its old counts.
Both contain scoped gate output with
local machine paths removed. It excludes private review transcripts and data.
The PR head and final independent review identify the final snapshot. The timed
receipt anchors compiled code and source hashes separately from later docs/data
commits. Build/test commands requested nice 10; sandboxed Cargo commands reported its
setpriority restriction. Clean performance runs and native/Bazel builds used
approved execution where nice 10 succeeded, serially with no active compiler
observed. No heavy agent probes overlapped them. This is ordinary desktop timing,
not thermal/frequency isolation.

Live dispatch, transport, UI/history, redaction, capture, held-out accuracy
and native parse cancellation remain integration work. See [review](review.md)
and [actual performance limits](performance.md).
