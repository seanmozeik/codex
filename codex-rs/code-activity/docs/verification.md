# Verification — 2 October 2026

Environment: macOS arm64, Rust 1.95.0, cargo-nextest 0.9.146, Python 3.14.7,
Node 26.10.0, Bazelisk 1.29.0 selecting Codex's pinned Bazel 9.0.0.
Official Codex base: `ca466061d64f0b44f416135c7fd06aa7af850bbc`.
Actual Tripwire comparison: `56f18dad3ac729db75e6d2437fa74b1b9a3c900f`.

| Check | Result |
|---|---|
| Standalone final `just verify` | 88 tests passed, zero skipped; 4 doctests; strict all-target Clippy, formatting, 350-line gate, docs and release build passed |
| In-tree parser + actual Codex adapter, nextest all targets including ignored fixtures | 100 passed, zero skipped |
| Final adapter assertion refactor, scoped nextest rerun | 12 passed, zero skipped |
| In-tree lifecycle/API doctests | 4 passed, including 3 compile-fail lifecycle/ID checks |
| In-tree changed-crate Clippy with pedantic/nursery and warnings denied | Passed, no suppressions |
| In-tree rustdoc warnings denied | Passed |
| In-tree all-target release build | Passed; inherited upstream thin-LTO profile |
| Native Bazel `//codex-rs/code-activity:all` | 13 targets passed, 11 Windows cross targets skipped on Mac |
| Final Bazel adapter rerun after test-only refactor | Passed |
| Bazel module lock update and error-mode verification | Passed |
| Bazel/Clippy policy consistency script | Passed |
| Workspace manifest policy script | Existing unchanged upstream failure: stale code-mode feature exception; same failure verified in pristine base |
| Rust files at most350 lines | All50 files passed |
| Frozen older Tripwire corpus | Byte-for-byte preserved;191 fully inspected input comparisons remain exact |
| Public content/provenance review | Synthetic fixtures/examples only; MIT attribution retained; no private transcripts/session payloads or detected credentials |

The 100 tests include 88 parser/lifecycle tests and 12 actual Codex boundary tests.
The function-golden test iterates 57 reviewed programs; test count is not fixture
count. Historical set comparison deduplicates identical operations, while newer
goldens assert ordered full effects and retained source evidence.

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
cargo nextest run -p codex-code-activity --all-targets --locked --no-fail-fast --run-ignored all
cargo test -p codex-code-activity --doc --locked
RUSTDOCFLAGS='-D warnings' cargo doc -p codex-code-activity --no-deps --locked
cargo build -p codex-code-activity --release --all-targets --locked
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
bazel test //codex-rs/code-activity:all --test_output=errors
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
The PR's final commit identifies the reviewed snapshot; live dispatch, transport,
UI/history, redaction, capture and held-out accuracy remain integration work.
