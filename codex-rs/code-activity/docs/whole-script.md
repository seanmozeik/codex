# Initial whole-script presentation contract

Receive the complete final post-hook source before interpreting it. `ScriptBuffer::append` retains decoded text without parsing or emitting rows. `finish` consumes that buffer into `CompleteScript`; only this completed type can reach `analyze_complete`. The host must preserve the exact original command separately for ordinary fallback and execution. A byte overflow aborts the entire buffer and drops its prefix.

A single unsupported fragment can mutate a binding, change control flow, or raise before a recognized operation. Consequently, the complete result is indivisible: any syntax or semantic gap, unresolved target/write mode, unmodeled child process, omitted snapshot, unsupported environment, or elapsed budget discards **all** operations and diffs, including earlier recognized rows. The filtered report is private and can only be borrowed immutably. Its serialized fallback has empty operations and diffs.

```rust
use codex_code_activity::{AnalysisLimits, Analyzer, EnvironmentAssumptions,
    Language, ScriptBuffer};

let mut source = ScriptBuffer::new(
    "call-id".into(), Language::Python, Some("/workspace".into()),
    EnvironmentAssumptions::FreshProcess, AnalysisLimits::default());
source.append("open('notes.txt').read()\n")?;
source.append("custom_mutation()\n")?;
let activity = Analyzer::new()?.analyze_complete(source.finish()?);
assert!(activity.ui_rows().is_empty());
assert!(activity.diffs().is_empty());
# Ok::<(), codex_code_activity::ActivityError>(())
```

`FreshProcess` asserts more than a new PID. The host must establish ordinary builtins/module resolution and recognized executable semantics, without injected bindings, startup hooks, aliases, or persistent state. Parsing does not verify those assertions. Use `PersistentOrUnknown` when they cannot be established. Standard-import initialization gaps are not waived. Literal path spelling is not proof of canonical runtime destination.

Strict-only guards reject reproduced unsupported behavior: Python assertions, comparisons, annotations and calls with more than two positional arguments; JavaScript complex assignment targets/updates and Deno references. These deliberate abstentions avoid inventing general language semantics. They do not prove that every other silent omission in the baseline interpreter has been eliminated. Successful syntax parsing, a gap-free report, and passing finite tests are distinct from effect correctness.

The default source limit is 1 MiB. The default 100 ms elapsed budget is shared across nested parsing, checked through tree-sitter progress callbacks and source boundaries, and checked after interpretation/diff work. Cancellation resets the parser before reuse. This is cooperative: it cannot preempt every semantic/diff operation and does not impose a hard wall-clock or total RSS ceiling. A host requiring a hard deadline needs an isolated cancellable worker and its ordinary fallback on worker failure. Buffer limits do not bound already-decoded caller or transport allocations.

Accepted rows are static intent. A predicted edit does not establish that it ran. Endpoint diffs are host-provided net observations; even an exact diff cannot establish which process wrote it. Keep runtime failures and observed changes separate from the preview.

This is an opt-in API in an unwired prototype. No Codex app-server events, renderer, dispatch, sandbox, Guardian, or approval code is changed. A future host adapter must route final source through this boundary while preserving its exact original command and existing authorization/execution path for both eligible and fallback results. No enrichment result grants permission. Existing partial diagnostics and streaming experiments remain available for research, but are not the initial shipping presentation contract.

## Guarded source enrichment

`SourceAnalysisGuard::shared()` owns one process-lifetime worker. It accepts one
complete source-only request at a time, with maximum logical source/call-ID/cwd
lengths of 32 KiB/256 bytes/4 KiB. Snapshots and unknown environments are rejected.
The caller's response wait is capped at 100 ms; zero disables enrichment. Jobs
carry the admission deadline, skip work when already expired, and receive only
the remaining cooperative budget after analyzer setup. The original shorter
analysis limit is preserved.

Every guard failure returns zero rows/diffs. Private response channels prevent
late replies from crossing requests, including repeated IDs. Timeout retains
admission until worker cleanup, and worker failure disables the service without
replacement. Back-to-back calls can observe `Busy` while the previous reply is
being cleaned up; callers must preserve ordinary presentation on that outcome.
These controls do not establish a hard RSS cap, preempt semantic work, terminate
CPU work on timeout, or isolate aborts/OOM/native process faults. The service does
not change the host panic hook or disclosure policy.

`analyze_guarded_codex_command` applies the same complete Bash-carrier boundary to
an actual `CommandExecutionItem`, checks source/ID caps before copying, and keeps
the exact original item. It is an opt-in bridge, not installed dispatch/rendering.
The ordinary Codex launch path inherits environment and can restore shell state;
that path cannot currently assert `FreshProcess`. Missing assumptions require
`PersistentOrUnknown` and ordinary presentation. A fresh PID alone is insufficient.

The complete-analysis path additionally supports literal computed members for
`fs.readFileSync`, `fs.writeFileSync`, `fs.appendFileSync`, and `fs.unlinkSync`.
Keys may use string literals, parentheses, and literal `+`. The receiver must
already be the known filesystem API; existing argument/mutation/source checks
still apply. Dynamic keys, unsupported options, other members and unresolved
surrounding code cause whole-script fallback. The diagnostic `Analyzer::analyze`
contract and its recognized subset are unchanged. Static intent remains distinct
from runtime completion, including when an early error prevents a later write.

Strict Python analysis also folds `replace(old, new)` on a known literal string
with exactly two ordinary known-string arguments. Bound-method aliases preserve
that receiver identity. The predicted UTF-8 output must fit 16,320 bytes before
allocation, including empty-pattern insertion at Unicode character boundaries.
This preserves literal-derived targets for existing file read/write/edit intent.
Unsupported arguments and oversized results cause whole-script fallback, even
when the result is unused. The diagnostic baseline, TypeScript replacement,
file-content transformations, import uncertainty, loops and the three-positional-
argument exclusion are unchanged.

### Literal Python loops

Strict Python analysis supports sequential `for` loops over directly written
lists or tuples of 1–16 decoded string literals. The target must be one bare
identifier. A separate body check admits straight-line expression statements and
single-name assignments; control flow, imports, nested loops, local functions,
callbacks and mutations through attributes or subscripts remain unsupported.
Empty or dynamic sequences fall back for the entire script.

Each iteration uses the preceding iteration's resulting bindings. The final body
state survives, including an assignment to the loop target. Calls are checked
against resolved builtin, handle and text/content values on every iteration;
spelling a method `replace` does not make an unknown receiver trustworthy. The
initial call subset covers builtin open/print, handle read/write and the
existing bounded text/content transformations. Existing source, value, operation
and traversal limits remain shared across iterations. Any gap discards all rows
and diffs, including effects found before a late failure. Diagnostic analysis
retains its previous conditional-loop behavior.

Loop analysis describes static file activity intent. It neither executes the
script nor calculates replacement bytes. Exact diffs still require separately
supplied host snapshots; the synchronous source-only guard rejects snapshots.

The initial loop slice also rejects any resolved binary `open` anywhere in a
strict Python source that contains a `for` statement or comprehension clause.
This includes origins before the loop and binary opens after it, even when that
particular handle is unrelated to the loop. The conservative whole-source rule
prevents erased binary Handle/Content provenance from being treated as text
through a prebound handle, content value or method alias. Known text handles
remain usable. Diagnostic analysis and strict Python sources without loops keep
their existing mode handling. This rule does not assert a general typed model of
Python byte strings, nor does it change import or environment assumptions.

For the same provenance reason, these loop-containing strict Python sources also
reject resolved Handle calls to `readlines`, `text` and `json` anywhere before,
inside or after the loop, including bound-method aliases. The existing shared
Handle model would otherwise return Content for a list-producing read or methods
that are not ordinary Python file-handle text reads. The check runs before
content transformation handling. Plain text-mode `read` remains supported; write,
close and truncate do not manufacture Content. This adds deliberate whole-source
abstentions even when the excluded read is unrelated to the loop. Diagnostic and
loop-free strict analysis retain their existing behavior.

The bounded grammar and typed value checks apply to the **entire** loop-containing
Python source, including prefixes and suffixes. Only straight-line expression
statements, single-name assignments and sequential admitted literal loops are
supported. With statements, function/class definitions, slicing/subscripts and
other control constructs are rejected even outside the loop. Calls, member
lookups, concatenation operands and identifier values must satisfy the same
proven file/text subset everywhere. This deliberately rejects conversions such
as `str(handle)` and JavaScript-like Content methods rather than propagating an
incorrect Python type. Explicit `close` is also rejected anywhere in these
sources because the current Handle model does not track closed state; context
managers are excluded for the same reason. Plain prebound text handles/content,
builtin aliases and sequential literal loops remain supported. These extra
abstentions are specific to strict Python sources containing loops and do not
change loop-free strict analysis or diagnostic behavior.

Within these loop-containing sources, a content operation that would append a
33rd represented transformation also forces whole-source fallback. This covers
replacement, normalization and content concatenation; the analyzer does not
silently stop recording transformations while retaining an eligible edit row.
