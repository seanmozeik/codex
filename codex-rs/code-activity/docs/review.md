# Scope and review record

The primary deliverable is source-backed activity suitable for a Codex UI:
versioned canonical effects, typed targets, spans, nested-source provenance,
explicit gaps and report-local identity. The security example is secondary and
isolated. It consumes the entire script before mock dispatch, cannot grant
permission, and demonstrates only selected lexical Tripwire rules.

The strict review used the author's complete personal Rust and strict-review
skills, including the applicable chapters and rule files. A fresh reviewer
examined source, tests, contract, classifier and benchmark implementation.
Findings were reported before repair. The integrator owned builds and commits;
the final independent reviewer did not author the repairs it accepted.

## Findings repaired

| Defect and consequence | Scoped repair and regression |
|---|---|
| Python generator bodies were treated as eager, inventing writes from an unconsumed generator | Separate lazy-generator analysis; only the outer iterable is evaluated, with an explicit consumption gap; `semantic_review` |
| Dynamic imports and asynchronous file reads could supply immediate APIs/text, inventing edits before resolution | Deferred imports/content require `await`; preserve `fs/promises` identity; callback reads and unencoded buffers do not become text; `semantic_review` |
| Imports, function seeds and locals could bypass binding limits, or expose an enclosing builtin after saturation | One bounded insertion path with persistent exhaustion; explicit gaps; `semantic_review` |
| Repeated embedded source aliases could clone payloads before source/byte limits were checked | Borrow interpreter text and check pending source capacity/bytes before cloning; `budget_review` and an internal queue regression |
| Repeated aliases could allocate large arrays, objects or argument lists before the stored-value limit applied | Check aggregate construction incrementally, including keys and keyword arguments; invalidate state on exhaustion; `resource_construction` |
| Skipped object spreads/computed children could retain stale file paths and cwd | Invalidate aliases and cwd whenever these children are skipped; six inert scenarios in `object_review` |
| Awaiting a modeled tool result unnecessarily erased independent JavaScript lexical bindings | Represent the recognized tool result as deferred opaque data; repeated nested-tool stress regression |
| A policy API accepting source and report separately could classify mismatched inputs | Private `ReviewedScript` owns the paired bounded source/report/decision; no external mismatched construction |
| Prefix matches could confuse harmless flag lookalikes with force/root-delete options | Exact selected options, operands and `--` handling; policy negative controls |

The filesystem dispatch extraction removes duplicated synchronous/asynchronous
logic while retaining explicit effect variants. No lint allowances or weakened
workspace lint settings were added. Rust sources remain at most 350 lines.

## Native performance review

The Python scanner's repeated suffix lookahead caused seconds of latency on a
311 KB comment fixture. A native guard now skips the suffix only when no external
token can be produced, retaining original comments and byte/point coordinates.
The 16 unchanged selected package files include the original generated parser;
only the C scanner changes. Complete MIT provenance, source hashes and the exact
patch accompany the vendored grammar. No Tripwire TypeScript runtime is added.

The renewed whole personal Rust and strict-review pass covers native/FFI
boundaries, every scanner return, original/incremental syntax-tree equivalence,
eight new language/contract/security regressions and Cargo/Bazel integration.
Independent review caught an evidence wording issue:639 incremental checks are
526 source-changing edits plus 113 no-ops. The driver/README/results now state
that split explicitly. [Actual performance](performance.md) includes the clean
matched ~956× fixture gain and the unchanged lower-indent quadratic control;
the patch does not replace deadline work.

## Compatibility and remaining work

The borrowed adapter compiles against the pinned real `CommandExecutionItem`.
Tests preserve its argv, cwd URI, process/status and existing `ParsedCommand`
fields, including explicit unsupported direct Python/Node/PowerShell input.
`protocol`, `app-server-protocol`, `core`, `app-server` and `tui` have no changes
relative to the official base. Their existing started/completed/output-delta
transport and UI behavior therefore remain unchanged; this is not evidence of a
new live transport integration.

Coverage is always partial or opaque. Operation order is discovery order, not
runtime order. Unsupported language semantics, native parser memory/time,
environment-dependent paths and arbitrary module/user-code effects remain
limits. The lexical demo does not reproduce all Tripwire policy behavior,
canonicalize filesystem paths or perform AI classification. No production
accuracy, hard latency bound or security completeness is claimed.

See [verification](verification.md), [performance](performance.md),
[output contract](output-contract.md) and [integration scope](codex-integration.md)
for actual receipts and remaining integration work. This remains a fork-local
discussion prototype under the pinned upstream contribution policy.
