# Tripwire comparison, selected ports and limits

Compared directly with Sean Mozeik's actual Tripwire 0.11.1 source at
[`56f18dad3ac729db75e6d2437fa74b1b9a3c900f`](https://github.com/seanmozeik/tripwire/tree/56f18dad3ac729db75e6d2437fa74b1b9a3c900f).
The standalone Rust comparison baseline was `e44abae7e8bb47b304f6857250e00a4ddeb5dd31`;
the original pre-refresh Rust base was `125f907ca4ea38ee3c59f20b60d46d9f7e0a4acc`.
This is a bounded independent Rust activity implementation, **not full current
Tripwire parity**. Tripwire owns protected-path policy and authorization; this
crate owns source-backed intent and optional host-snapshot observations.

## Audit evidence

Two independent review lanes inspected actual Tripwire parser sources/tests
and the Rust implementation. The language review built the pinned Rust baseline
separately and compared **116 synthetic inert programs: 43 Python, 58 plain JS,
15 explicitly typed TS**. The shell review inspected option ownership, redirect
input, cwd joins, startup state and carrier identity with inert CLI probes.
No submitted program or child command was executed by either analyzer.

The review read `src/lib/code/` scope/binding/argument/function/callback/control,
file/transfer/path/library/data/carrier implementations and corresponding
`test/inline/` suites. Shell review covered `src/lib/bash/` analysis/cwd/carriers,
wrapper metadata, startup handling, and bash/cwd/wrapper/forwarding/workdir tests.
These probes diagnose semantics; they are not a held-out accuracy benchmark.

The older frozen corpus in `tests/fixtures/tripwire.json` contains 355 commands,
273 embedded inputs, 191 fully inspected reference inputs and 223 operations.
It remains byte-for-byte unchanged. Exact normalized operation-set comparison
covers the 191 fully inspected inputs; multiplicity and ordering are tested
separately. Four historical unsupported function fixtures now require exact
reviewed deletion sets. This historical corpus alone cannot prove current parity.

`tests/fixtures/functions.json` contains 57 hand-reviewed inert goldens. They
assert ordered effects, modes/cwd/repeated effects and source evidence. A fixture
marked supported rejects any unexpected gap. `language_review.rs`,
`shell_review.rs` and `budget_review.rs` add adversarial assertions rather than
blessing the reference analyzer's output.

Tripwire's parser-only data/evaluation suites passed 62 tests. Its broader
inline-suite attempt encountered 14 file-level import failures because the local
checkout lacked `effect`; it was not reported as a passing full Tripwire gate.
Original source checkouts stayed clean; no dependency install was used to conceal
that limitation.

## Feature matrix

“Gap” means explicit uncertainty/abstention, not permission or absence of activity.
Rows describe selected tested forms; neither parser is a complete language engine.
All Tripwire paths below refer to the pinned revision above.

| Feature | Rust behavior in this PR | Current Tripwire comparison / evidence |
|---|---|---|
| Python imports/aliases/dotted imports | Selected standard module roots and aliases; initialization gap | `code/imports.ts`, `inline/import-contracts.test.ts` |
| JS/TS named/namespace/require/dynamic built-in imports | Selected builtin namespaces and renamed bindings | `code/imports.ts`, `inline/closures.test.ts`; some Rust typed forms exceed TW |
| Type-only imports | Erased bindings cannot grant runtime API authority; mixed imports retain runtime names | Audit found the same false authority in both baselines; repaired with adversarial twins |
| Lexical scalar captures and JS let/const blocks | Retained scope identity, updated captures, block shadows | `code/scope.ts`, `inline/scope.test.ts`, `inline/closures.test.ts` |
| Python function-local names | Imports/for/with names seeded before use; unsupported binders abstain | Avoids reviewed TW use-before-local-import false literal |
| Python global/nonlocal and JS var hoisting | Gap/preflight abstention | TW supports selected nonlocal; reviewed var result is not a sound oracle |
| Functions, lambdas, typed arrows, named expressions | Called local bodies, aliases, parameters and nested calls | `code/analyze.ts`, `bindings.ts`, `arguments.ts`, `inline/closures.test.ts` |
| Returned closures | Independent retained lexical scopes and late captures | TW parent-linked scopes; no general persistent REPL state in Rust |
| Arguments/defaults/rest/destructuring/expansion | Required/duplicate Python args validated; defaults/rest/destructuring/*/**/... remain gaps | TW has broader bounded normalization; Python definition-time default effects retained, JS defaults lazy |
| Return/recursion/async | Definite returns stop; conditional returns unknown; cycles/16 active calls bounded; JS results deferred until await | TW conservative policy differs; unawaited async values must not become literal paths |
| Branches/literal loops/comprehensions | Conditional intent and state joins; literal bounded loops and selected single-generator comprehensions | `code/control.ts`, `inline/control-flow.test.ts`; broader syntax is a gap |
| Exceptions/classes/generators/Python async | Gap, including preflight rejection where needed | TW supports more try/finally and bounded Python class contracts; not ported |
| JS map/forEach callbacks | At most128 literal items, narrow synchronous syntax, mutation/opaque callback abstention | `code/callbacks.ts`, `inline/literal-callbacks.test.ts` |
| Other callback families | filter/reduce/some/every/find/flatMap, Python map/filter/key callbacks and scheduled callbacks remain gaps | TW supports broader bounded callback contracts |
| Implicit callback/coercion hazards | JSON revivers/replacers, nested toJSON/toString functions, regex/string replacements and Python key/hooks invalidate state | Audit repairs prevent an options gap from retaining stale literal targets |
| Shared containers/heap alias mutation | No general mutable heap; detected/opaque mutation invalidates precise values | TW has broader container contracts and mutation tracking |
| File reads/open/write/edit/delete/truncate/list | Selected pathlib/open/Node/Deno/Bun APIs; explicit modes and read-origin transforms | `code/operations.ts`, `file-options.ts`, `inline/resolved-paths.test.ts` |
| JSON and ordinary data | Selected inert JSON/indexing/serialization/string transforms; gaps for unsupported contracts | `code/data.ts`, `builtins.ts`, `inline/data-operations.test.ts` |
| Transfers/archive/metadata/mkdir | Rename/copy/move/symlink/ZIP/stat/existence and many mkdir APIs are gaps | TW `operations.ts`, `zipfile.ts`, `python-library.ts`, transfer/library suites |
| Path joins | Separate Node lexical normalization and Python symlink-sensitive parent spelling | Rust retains link/../file for Python; TW output is not blindly copied |
| Filesystem-backed resolution | Pure source analyzer does not resolve host realpaths | TW `resolved-paths.ts` has policy/host checks; remote-safe adapter needed |
| Process argv/nested literals | Literal ProcessRun plus restricted option-free embedded interpreter source | `code/calls.ts`, `carriers.ts`; child process internals remain gaps |
| Process cwd/options/submitted interpreter argv | Context-changing options prevent speculative child parsing; no general sys.argv/process.argv plumbing | TW signature/context contracts are broader; high-value future port |
| Typed tools.exec_command wrappers | Literal/shorthand arguments, workdir ownership, nested source spans; unsupported shell/login context abstains | Codex-specific Rust support; not a TW language-analyzer primitive |
| Shell carriers | Versioned Python/PyPy, Node/Bun/tsx/ts-node, Deno eval, uv option ownership, literal heredoc/here-string/inline source | `code/carriers.ts`, `uv.ts`, interpreter-argument suites |
| Deno flags | eval-subcommand owns print flags; invalid Node-style deno -e stays opaque | Runner-specific repair; generic flag tables are not a reliable oracle |
| Shell cwd/pipelines/alternatives | Consume cd --; unsupported options unknown; sibling isolation and conservative outgoing joins | `bash/cwd.ts`, `bash/analyze.ts`, cwd tests; no filesystem existence read |
| Shell redirects | Competing/non-stdin literal input abstains; stdout/file redirects keep explicit effects gap | `code/carriers.ts`, `bash/analyze.ts`, security regressions; no full fd simulation |
| Expansions and executable shadowing | Quoted literals distinct from tilde/brace/glob/dynamic words; unmodeled aliases/functions/source prevent precise carrier assumptions | TW typed-word/environment model is broader |
| Startup and wrappers | Inline env assignments skip speculative source; env location changes remain uncertain; eight wrapper steps bounded | TW startup/wrapper specs are broader; most credential/package/env/remote wrappers remain opaque |
| Shell functions/substitutions/SSH/decoded pipelines | Explicit gaps, no local capture of speculative remote effects | TW `bash/carriers.ts`, `pipeline-inputs.ts`, forwarding tests; not ported |
| Limits and lifecycle | Bounded sizes/visits/functions/frames/scopes; stream revisions rebuild semantics; captured changes separate | TW evaluation bounds inspire deterministic limits, not shared authorization semantics |

## Selected later provenance

- [`2a26f305`](https://github.com/seanmozeik/tripwire/commit/2a26f30556074d24b533b919a539502189fa6bd9): closure/scalar scope architecture.
- [`d01df91c`](https://github.com/seanmozeik/tripwire/commit/d01df91c3e4a98476fdc6d67e8d7da8761480487): local binding and argument correctness reference.
- [`001123c1`](https://github.com/seanmozeik/tripwire/commit/001123c1fa7ff244276773ab0eb846becef4a1ad): literal callback contracts.
- [`ca793c5a`](https://github.com/seanmozeik/tripwire/commit/ca793c5aad1a44294c7364edbdfcd6a77d972363): mutation/loop-jump hazards.
- [`272bfb76`](https://github.com/seanmozeik/tripwire/commit/272bfb76ed80fab5bcc4b3d1b021c08f7d1cd921): evaluation budgets.
- [`a314d91c`](https://github.com/seanmozeik/tripwire/commit/a314d91c1d5c6d977ec105c750a4ea463e92d25e): shell cwd/wrapper comparison.
- [`77d99f38`](https://github.com/seanmozeik/tripwire/commit/77d99f38dc71f6e6ef8f6e0b6dfbc851a1e89239): forwarded argv comparison.

Tripwire's complete MIT license and copyright Sean Mozeik 2026 are preserved in
[NOTICE](../NOTICE). No TypeScript implementation, protected-path policy,
execution engine, bypass rules, private transcripts or real-session fixtures
were copied into this PR. The synthetic older corpus is attributable behavioral
reference material. The crate inherits Codex's Apache-2.0 metadata.

## Next useful ports

Prioritize bounded argument/default/destructuring normalization and common
synchronous callbacks, then properly typed transfer/metadata intent and child
invocation context. Broader exceptions/classes/shell evaluation require separate
bounded models. Keep callback hooks, shared mutation, startup state and host
resolution uncertain until reviewed. Preserve raw source spelling/evidence and
measure target/effect precision on a hand-labelled held-out set before live UI
integration.
