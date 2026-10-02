# Structured output and consumer contract

The primary deliverable is an output shape: source-derived file/process records
that a UI can display and a security hook can inspect without reparsing snippets.
`ActivityContract` is an additive **local prototype** with the discriminator
`codex.codeActivity.v1`. The existing `Report`, lifecycle JSON, actual Codex core
item, app-server `ThreadItem`, and item started/completed/output-delta events are
unchanged. This PR does not register a production protocol or attach new UI events.

The checked-in [JSON Schema](../tests/fixtures/output-contract.schema.json)
describes the complete outbound envelope and current effect variants. Rust
`ContractSchema` deserialization rejects unknown version strings; there is no
general inbound envelope decoder or untrusted JSON validator in this prototype.
JSON emission/decoding in tests demonstrates the outbound shape, while typed
consumers borrow trusted local reports. External consumers must validate versions,
fields, references and resource limits before accepting network data.

## Version one invariants

| Field | Consumer meaning |
| --- | --- |
| `schema` | Exact `codex.codeActivity.v1`; incompatible field meanings or action semantics need a new discriminator. |
| `callId` | Opaque host identity, with no path semantics. |
| `analysis.type` | `supported` has a report; `unsupported` has a carrier/budget reason. An unsupported carrier is never an empty successful analysis. |
| `coverage` | Always `partial` or `opaque`; even a report without explicit gaps is not exhaustive. |
| `records` | Original ordered operations, with multiplicity retained. This is deterministic **report discovery order**, not global execution order. Outer records are collected before nested source analysis. |
| record `id` | Zero-based operation-table identity scoped to this report/revision. Replacement revisions may reuse IDs for different records; do not merge by ID across revisions. |
| `basis` | Always `staticIntent`; completion status, stdout or predicted writes never become file-change receipts. |
| `sources` and `evidence` | Report-local source IDs; nested source parent spans point to the containing argument/heredoc. Spans use decoded UTF-8 byte offsets, exclusive end, one-based lines and byte columns. Child offsets belong to the child source, not the outer source. |
| `target` | Literal spelling and optional executor cwd, or explicit `unresolved`. Literals do not establish canonical paths, symlink identity or runtime values. |
| `unresolved` | Preserved diagnostics and evidence spans. Reasons are human text, not stable policy discriminators. |

No deduplication, path normalization or success claim is added by the envelope.
Host-supplied endpoint diffs remain in the legacy report/lifecycle APIs; they are
intentionally excluded from the source-intent envelope. Source text and raw item
contents require host redaction/disclosure policy before client delivery.

`ui_rows()` maps each original effect to a typed read/list/open/write/append/edit/
delete/truncate/run action, keeps the canonical effect details and evidence by
reference, and surfaces `argumentKnowledge: unknown` for unresolved targets or
process arguments. A UI must show the top-level partial/opaque/unsupported status
alongside rows. A bare empty row list means neither no effects nor safety.

## Tested representative programs

`tests/contract.rs` exercises Python returned closures, TypeScript typed callback
parameters and literal `map`, JavaScript returned closures, Node inline append,
shell-to-Python deletion, and TypeScript nested tool → shell → Python reads.
Each flows through actual analysis → versioned JSON emission/JSON decoding →
typed UI rows, retaining source provenance and static basis. Separate fixtures
check ordered duplicates, stable repeated JSON bytes, unknown targets/gaps,
unknown-version rejection, and supported/unsupported actual Codex core items.
The actual item retains its status, argv, legacy parsed commands and executor URI.
These tests do not execute the submitted programs.

## Whole-script policy demonstration

`policy::review_script` parses the entire final decoded script **before** making
one decision. `ReviewedScript` privately binds that source, report and decision;
callers cannot pair a benign report with a replacement dangerous script. One
bounded source copy retains raw classifier context. Inputs over the 1 MiB source
budget are not copied/retained and explicitly escalate. No policy decision has
an `allow` variant: selected hits block, known uncertainty escalates, and a
gap-free partial report still requires independent authoritative permission.

The classifier is deterministic mock code, not an AI classifier and not a
complete Tripwire port. It receives the source-bound report, scans all structured
operations, and ties every hit to its operation ID. It uses these inspected rules
from MIT-licensed Tripwire 0.11.1, commit
`56f18dad3ac729db75e6d2437fa74b1b9a3c900f` (copyright attribution in `NOTICE`):

| Local rule | Tripwire origin | Port scope |
| --- | --- | --- |
| `read-env`, `env-file` | `src/rules/read-protect.ts`, `src/rules/path-protect.ts` | Literal basename `.env` or `.env.*`, including `.env.example` like the original matcher. |
| `read-ssh`, `ssh-dir` | Same path protection modules | Literal POSIX path component `.ssh`; read matching also includes the directory itself, a conservative extension. |
| `root-deletion` | `src/rules/bash-deny.ts` (`rm-rf-root`), `src/rules/embedded-code.ts` (`safeDeletion`) | Literal `rm` argv with `/` and `-rf`/`-fr`/`-Rf`/`-fR` before `--`, plus every recognized file deletion of literal `/` including Python/JS equivalents. |
| `git-force-push` | `src/rules/bash-git.ts` (`handlePush`) | Literal exact `git push` argv, `-f`, `--force`, exact `--force-with-lease` or `--force-with-lease=…`, before `--`. Invalid prefix lookalikes do not hit. |

This port deliberately omits filesystem canonicalization, symlink/home expansion,
startup inspection, Git global-option parsing, combined flags, arbitrary shell
strings, custom configuration, safe deletion scopes, bypass comments and the
other Tripwire rules. Equivalent Python/JS/TS file operations are classified via
shared typed effects rather than duplicated language-specific policy branches.
Known rule misses do not authorize execution. An external host must enforce its
actual permissions, sandbox, cancellation and runtime controls independently.

`tests/policy.rs` puts a dangerous operation **after** a benign operation in
shell, Python, TypeScript, Node and nested tool programs, then confirms the whole
script blocks and the mock executor receives **zero calls**, even with simulated
permission. Dynamic targets, recursion, opaque code, malformed source and source
budget exhaustion escalate with zero calls. Comments/string rule mentions do
not fabricate hits. The ordinary-read fixture remains undispatched without an
independent simulated approval. `MockExecutor` only counts calls; it cannot
launch processes or read/write files. Production pre-tool hooks must analyze
after rewrites and dispatch exactly the reviewed immutable input.

Run the standalone synthetic demonstration with:

```sh
cargo run -p codex-code-activity --example consumer --locked
```

It prints the envelope, typed rows, policy result and zero mock executor calls.
The source is synthetic and inert. Full parse/emission/UI/policy cost belongs in
the performance evidence; grammar recognition alone does not measure this flow.
