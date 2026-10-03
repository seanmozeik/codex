# Activity contract

`ActivityContract` wraps a trusted local report in the outbound envelope
`codex.codeActivity.v1`. It is separate from Codex's existing command items and
app-server protocol. The [JSON Schema](../tests/fixtures/output-contract.schema.json)
describes the emitted envelope; `ContractSchema` rejects unknown version strings.
This crate is not a general validator for untrusted network envelopes.

| Field | Meaning |
|---|---|
| `callId` | Opaque host identity, without path semantics. |
| `analysis.type` | `supported` contains a report; `unsupported` contains a carrier/budget reason. Unsupported input is not empty successful analysis. |
| `coverage` | Always `partial` or `opaque`, even without explicit gaps. |
| `records` | Effects in deterministic discovery order, retaining duplicates. This is not global runtime order; outer records precede nested-source analysis. |
| Record `id` | Zero-based identity local to this report/revision. Do not merge records by ID across replacement revisions. |
| `basis` | Always `staticIntent`; predicted writes and completion status are not file-change receipts. |
| `sources`, `evidence` | Report-local source IDs and spans. Nested parent spans identify the containing argument/heredoc. Offsets are decoded UTF-8 bytes with exclusive ends, one-based lines and byte columns. |
| `target` | Literal spelling with optional executor cwd, or `unresolved`. A literal does not establish canonical path, symlink identity or runtime value. |
| `unresolved` | Preserved diagnostics and source spans. Human reason strings are not stable policy discriminators. |

`ui_rows()` maps canonical effects to typed read/list/open/write/append/edit/
delete/truncate/run rows, borrowing their effect details and evidence. Unknown
targets or process arguments retain `argumentKnowledge: unknown`. Clients must
show partial/opaque/unsupported status alongside the rows. Empty rows mean
neither no effects nor safety.

No deduplication, path normalization or success claim is added. Endpoint file
diffs belong to the separate lifecycle API and are excluded from this source-
intent envelope. Hosts must redact source/item contents before client delivery;
external consumers must validate versions, references and resource limits.

## Whole-script policy example

`policy::review_script` analyzes the entire final source before making one mock
decision. `ReviewedScript` privately owns the paired source, report and decision,
preventing callers from substituting different source after classification.
Oversized source is not retained and escalates explicitly.

The example checks selected literal `.env`/`.ssh` accesses, root deletion and
force-push arguments. It does not canonicalize paths or implement full Tripwire
policy. Decisions can block, escalate or require authoritative permission; none
can allow execution. Unknown semantics must not bypass runtime permissions.
`MockExecutor` counts calls and cannot launch processes or access files.

```sh
cargo run -p codex-code-activity --example consumer --locked
```

The example prints a contract, typed rows and a block decision for a sensitive
write following an ordinary read, with zero mock dispatch calls. Its source is
inert. Tests cover late dangerous operations, unknown targets, malformed input,
ordered duplicates, version rejection and nested-source provenance.
