# Rust standards

Apply Sean's Rust skill before changing this crate. On this machine its source is
`rust-skill/SKILL.md in this handoff`.
Read the relevant reference chapters and rule files, not just its index.
The matching Talon `.claude` skill has the same content.

Use Rust 2024 and the pinned Rust 1.95 toolchain for Codex compatibility.
The crate name is `codex-code-activity`; keep the standalone binary `code-activity`.
Inherit package metadata, dependencies and lints from the workspace. Preserve
upstream's thin-LTO release profile and individual imports. Do not add Cargo
features or optional dependencies; Codex prohibits new workspace crate features.
Use `pretty_assertions::assert_eq` in tests. Keep the upstream denied Clippy
lints alongside Sean's stricter local gates. Compare upstream claims against the
pinned source in `MAINTAINER_REVIEW.md`; local checks are not upstream CI.
Add dependencies with `cargo add`; write full major.minor.patch versions.
Use `thiserror` for library errors, `eyre` for executable error context, and
`fs-err` for filesystem operations. Use `time` rather than `chrono` if calendar
time is needed; use `std::time::Instant` and `Duration` for measurements.

Keep identifiers and finite domain states typed. Preserve the consumed
stream/final/prepared lifecycle and its compile-fail examples. Do not infer
execution success from source, stdout text, or unchanged file contents.

Use explicit public re-exports. Document public APIs and their error cases.
Use non-exhaustive types where the API can grow. Closed phase markers remain
closed. Preserve the JSON contract when changing Rust implementation types.

Run `just verify` before reporting completion. It checks formatting, compilation,
strict Clippy, the 350-line limit on all Rust files, tests including the fixed
Python/Node runtime fixtures, doctests, rustdoc and release builds.
Use `just check` for a shorter implementation loop.

Fix warnings through refactors. Do not weaken lint settings or add suppressions
to make a check pass. Preserve unrelated changes. Do not push or publish without
Sean's instruction.
