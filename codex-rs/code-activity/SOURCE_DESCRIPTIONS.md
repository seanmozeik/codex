# Complete source references before execution

`Analyzer::describe_complete` and `SourceAnalysisGuard::describe` describe a
complete, bounded source with a closed grammar. The UI label is **Source
references**. A `writeCall` means the source spells a supported write call through
a recognized source binding. It does not mean the callable is the standard API,
that the call will run, that a path resolves to a particular file, or that a write
will succeed. Unknown ambient runtimes therefore need no invented trust profile.

This is a separate API from `WholeScriptActivity`. It has no effects, diffs,
snapshots, permission decisions or execution status, and cannot supply existing
file-change rows. `completeSyntax` describes grammar completeness only. Retain the
original command and existing sandbox, approval and Guardian behavior.

Every supported outer construct and decoded child must pass before any reference
is returned. Unsupported syntax, malformed input, unresolved origins or a budget
failure discard the entire result, including its source table. A known Python or
JavaScript carrier never becomes an opaque command when its child is unsupported.

## Supported syntax

- Python: `from pathlib import Path` with an optional alias; literal paths; simple
  data assignments and aliases; closed data reassignment and tracked-text `+`;
  `read_text`, `write_text`, `open`, `read`, `write`; immutable local aliases of
  the `open` spelling; literal `replace` chains with an optional canonical
  nonnegative decimal count; literal UTF-8 encoding options; one
  `with open(...) as name` context, including a tracked open alias; simple `print`
  calls. A local Path-call binding can be reassigned only to another Path-call
  origin. Earlier aliases retain their original literal path association.
- JavaScript: leading `require('fs'|'node:fs')`, namespace or default imports;
  direct inline literal `require` receivers; literal UTF-8 `readFileSync` and
  text `writeFileSync` with literal UTF-8 or omitted encoding; const/let data
  aliases; closed let reassignment; literal `replace`/`replaceAll`; simple output
  calls. Computed filesystem method keys accept only bounded literal strings,
  parentheses and literal string concatenation, with no coercion or dynamic keys.
- Shell: a finite list of literal command invocations, sequential statements,
  exact inline interpreter carriers, a literal `exec` directly before an interpreter,
  and quoted heredocs. Expansions, compound
  shell control and unknown carriers fall back.
- Orchestration: direct awaited `tools.exec_command`, literal `cmd`, `workdir`,
  `max_output_tokens` and `yield_time_ms`, literal `login`/`tty` booleans, and
  untagged template strings without substitutions; literal known `shell`,
  `sandbox_permissions`, `justification` and `prefix_rule` options; const result
  bindings; `text` consumers;
  direct `Promise.all`/`allSettled` arrays and narrow result-display loops.

Unknown imports/calls, dynamic paths, helpers, arbitrary control flow, callbacks,
module or handle rebinding, property mutation and unsupported options fall back.
Option references describe requested source spelling only. They do not approve
escalation or assert effective permissions. Objects and literal arrays retain the
eight-item bound even when more option names are recognized.

The implementation does not evaluate captured source, import modules, expand
shell text, resolve paths or compute replacement bytes. Python and JavaScript
language rules and replacement behavior differ; the output names their syntax.
Concatenated Python data does not become a path literal, and mixed data origins
remain unattributed. Replacement counts are recognized syntax, not evaluated
transformation outcomes. None of these forms establishes standard module or
builtin identity, effective encoding, successful I/O or actual file contents.

Literal paths retain decoded spelling, including `./` components. Source spans
refer to decoded sources; parent spans identify their containing carrier. The
host must retain the original complete source and its decoding context to resolve
those spans. Reference order is a source-description order, not an execution trace.

## Bounds and integration

The input constructor limits source bytes and call identity. The collector caps
aggregate decoded sources at 32 KiB, 32 sources, eight nested carriers, 256
references and 1 MiB of logical retained reference data. Each parse has node/depth
bounds; language readers have binding/literal limits. All children share one
cooperative elapsed budget. These are logical bounds, not a hard RSS/CPU limit.

`SourceAnalysisGuard::describe` uses the same single admission, worker, analyzer
and absolute deadline as the existing analysis APIs. A timed-out job continues to
occupy admission until it finishes; it cannot start a second worker or attach a
late result to a later call. Guard failure exposes no partial description.

The `codex_protocol_demo` example returns the source description in its flushed
`prepared` response before its external controller executes the original argv.
It separately keeps unknown-runtime effect rows empty. Completion reports only
host-selected before/after snapshots as observed changes. A standard module and a
shadow module may produce identical source references and different observations.
This example exercises actual Codex protocol types; it is not a production TUI,
executor, Guardian or transport integration test.
