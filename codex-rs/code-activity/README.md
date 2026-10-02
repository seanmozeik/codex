# Code activity: in-tree discussion prototype

This crate turns generated Python, JavaScript/TypeScript and shell source into
bounded, source-backed static activity records. It recognizes file reads,
writes, edits, deletion/truncation, listings and process launches, including
literal nested tool and interpreter calls. It never executes submitted source.

This PR is a review surface in Sean's fork of Codex. The crate compiles against
Codex's actual workspace and has a tested adapter for its real
`CommandExecutionItem`. It is **not wired into core dispatch, app-server events,
thread history or the UI**, and is not a shippable integration.

- [Tripwire parity and provenance](docs/tripwire-parity.md): actual current
  source/test audit, selected ports, differences and explicit unsupported cases.
- [Codex integration scope](docs/codex-integration.md): actual adapter, proposed
  lifecycle, remaining transport/environment/privacy work.
- [Verification](docs/verification.md): commands, counts and limitations.
- [Output contract](docs/output-contract.md): versioned records and tested UI
  consumer, with explicit partial coverage and provenance.
- [Performance evidence](docs/performance.md): reproducible inert stress inputs,
  end-to-end timing, common original-parser comparison and isolated peak RSS.
- [Synthetic adapter output](examples/codex_item.output.json): original Codex item
  plus independent static intent; no observed changes or new public API.
- [Lifecycle contract](docs/lifecycle.md): stream replacement, preparation,
  host capture and completion.

## Try it

From `codex-rs` with the repository's pinned Rust 1.95.0 toolchain:

```sh
cargo run -p codex-code-activity --example codex_item --locked
cargo run -p codex-code-activity --bin codex-code-activity --locked < code-activity/examples/python-edit.jsonl
cargo nextest run -p codex-code-activity --all-targets --locked --run-ignored all
cargo test -p codex-code-activity --doc --locked
```

The first example analyses an inert, synthetic command item. The JSONL CLI is
also pure source analysis. The lifecycle example and two opt-in runtime tests
execute only fixed, trusted fixtures in temporary directories; they do not
execute model/user input.

The core adapter uses Codex's shell extractors on original argv, preserves the
whole borrowed command item and existing `ParsedCommand`, and adds a separate
supported/unsupported static result. Direct Python/Node argv and PowerShell
explicitly abstain at this adapter boundary, although the standalone analyzer
can accept Python/TypeScript source directly. Bash/Zsh/Sh carriers receive
best-effort Bash grammar analysis with gaps. Executor cwd remains an unchanged
`PathUri` on the original item; the parser starts with unresolved cwd rather
than inventing host-native paths from a URI. Explicit source cwd changes are
conditional intent. Raw demo output needs host disclosure rules before delivery
to clients.

## Meaning and limits

Every source-derived operation has `basis: staticIntent`; coverage is always
partial or opaque. A recognized write is not a receipt that it ran. Operations
have source spans and report-local IDs; nested decoded sources point to their
containing source span. Sources and aliases are rebuilt on every stream revision
so stale previews can be replaced or retracted.

The optional standalone capture lifecycle compares bounded host snapshots. Its
`hostSnapshotsOnly`/`captureInterval` attribution cannot prove which process
changed a file, and equal endpoints cannot exclude intermediate changes.
`LocalFiles` is not an executor-aware Codex capture adapter and is not used by
the Codex item bridge.

The parser limits decoded source to 1 MiB, operations and uncertainty records to
256 each, nested sources to 64, embedded depth to 4, syntax depth to 64 and
semantic visits to 50,000 per source. Abstract values, local functions, frames,
scopes and literal callback iteration are separately bounded. These are size
and traversal bounds, not a wall-clock deadline or complete language sandbox.

Module meanings are assumed only for recognized standard/builtin imports.
Module initialization, persistent REPL state, arbitrary user code, dynamic eval,
general shared container mutation, full shell evaluation and remote execution
remain unmodeled. Defaults/rest/destructuring, classes/methods, Python
async/generators/global/nonlocal, JS var hoisting and most callback families
remain explicit gaps. Unsupported input must never be treated as permission to
execute or as proof that no activity will occur.

## Attribution and discussion

Tripwire is Sean Mozeik's MIT-licensed policy inspector. This is an independent
Rust activity implementation informed by its parser architecture and synthetic
regression scenarios, not a copy of its policy decisions or a full port.
The isolated whole-script mock policy example separately reimplements a small
documented set of Tripwire lexical rules; it cannot authorize execution and does
not change Codex's permissions or live dispatch.
[NOTICE](NOTICE) preserves its complete MIT attribution; the crate inherits
Codex's Apache-2.0 license metadata. No private transcripts, real session
payloads or internal benchmark corpora are included.

Current official Codex [contribution policy](https://github.com/openai/codex/blob/c5d242fa7907bff1b7a7e26e95febc548c0a6963/docs/contributing.md#L5)
declines external code PRs. This fork-local PR is for Sean to share in his own
team conversation; it makes no claim of upstream acceptance.
