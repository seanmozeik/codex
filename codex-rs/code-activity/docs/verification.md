# Verification — 2 October 2026

Environment: macOS arm64, Rust 1.95.0, cargo-nextest 0.9.146, Python 3.14.7,
Node 26.10.0, Bazelisk 1.29.0 selecting Codex's pinned Bazel 9.0.0.
Official Codex base: `ca466061d64f0b44f416135c7fd06aa7af850bbc`.
Actual Tripwire comparison: `56f18dad3ac729db75e6d2437fa74b1b9a3c900f`.

| Check | Actual current result |
|---|---|
| In-tree nextest, all targets including ignored fixed runtime fixtures | **139 passed, zero skipped**, zero failures/retries in final run; 44.75 seconds with two test threads |
| In-tree lifecycle/API doctests | **4 passed**, including three compile-fail lifecycle/ID checks |
| Changed-crate all-target Clippy, warnings denied plus pedantic/nursery | Passed; no suppressions or weakened lints |
| Formatting and warning-denied rustdoc | Passed; stable rustfmt retains the unchanged upstream nightly-setting warning |
| In-tree all-target release build | Passed; inherited thin-LTO profile; two low-priority build jobs |
| Native Bazel `//codex-rs/code-activity:all` | **19 targets passed; 17 Windows cross targets skipped** on macOS |
| Bazel module-lock error mode | Passed, no dependency changes in this review round |
| Bazel/Clippy policy consistency script | Passed |
| Workspace manifest policy script | Existing unchanged upstream failure: stale code-mode feature exception; earlier reproduced in pristine base |
| Source-size check | All **64 Rust files** are at most 350 lines: 63 crate/example/test files plus the separate 115-line diagnostic artifact |
| Stress and timed evidence | **111 inert cases / 222 pipeline measurements**, 32 original/current common comparisons, raw durations and 12 isolated consumer RSS logs; native parse-only diagnostic |
| Standalone diagnostic compilation and strict Clippy | Passed against the original locked grammar/profile; no source execution |
| Measured source/data integrity | All measured Rust/dataset/driver hashes still match; exact expanded corpus hash and 222 sample counts verified |
| Codex compatibility | Real item adapter tests pass; no changes to core, app-server, protocols or TUI relative to official base |
| Original checkouts | Parser `125f907…` and Tripwire `56f18da…` remain clean and untouched |
| Frozen older Tripwire corpus | Byte-for-byte preserved; 191 full operation comparisons remain exact; a generator uncertainty exception is narrow and explicit |
| Public content/provenance | Synthetic inputs only; MIT Tripwire attribution retained; source-only original baseline retains its Apache-2.0 license |

The current 139 tests include the original regression suite, 12 real Codex item
boundary tests, contract/policy consumers, semantic/resource repairs and scaling
stress. Test count is not fixture count: one function-golden test iterates 57
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
cargo clippy -p codex-code-activity --all-targets --no-deps --locked -- -D warnings -W clippy::pedantic -W clippy::nursery
CARGO_BUILD_JOBS=2 nice -n 10 cargo nextest run --test-threads 2 -p codex-code-activity --all-targets --locked --no-fail-fast --run-ignored all
cargo test -p codex-code-activity --doc --locked
RUSTDOCFLAGS='-D warnings' cargo doc -p codex-code-activity --no-deps --locked
CARGO_BUILD_JOBS=2 nice -n 10 cargo build -p codex-code-activity --release --all-targets --locked
cargo run -p codex-code-activity --example codex_item --locked
```

`--no-deps` scopes the additional pedantic/nursery lint groups to this changed
crate rather than imposing new policy on unchanged upstream path dependencies.
The crate inherits Codex's existing dependency/lint configuration and explicitly
forbids unsafe Rust. The separate standalone gate retains its stricter original
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

[Verification log](verification.log) contains successful scoped gate output with
local machine paths removed. It excludes private review transcripts and data.
The PR head and final independent review identify the final snapshot. The timed
receipt anchors compiled code and source hashes separately from later docs/data
commits. Live dispatch, transport, UI/history, redaction, capture, held-out accuracy
and native parse cancellation remain integration work. See [review](review.md)
and [actual performance limits](performance.md).
