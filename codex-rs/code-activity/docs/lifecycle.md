# Streaming and observation contract

The library emits three types of view. The execution host owns dispatch, permissions, process lifetime and transport.

| Phase | Evidence | Client action |
|---|---|---|
| `pending` | Current source interpretation, with a revision | Replace the previous pending view for this call |
| `prepared` | Frozen final report and summaries of original files | Replace the pending view; show that execution is still unproved |
| `completed` | Terminal tool result and file comparisons | Show the exit status and each observed change separately |

The library never runs model code. Only the fixed demo and the opt-in runtime tests start Python or Node.

## Streaming

Feed decoded source fragments to `ActivityStream::append`. A fragment must be valid UTF-8. A network decoder must retain incomplete UTF-8 bytes, JSON escapes and surrogate pairs until it can emit complete source characters. The prototype does not include that transport decoder.

Each append edits the previous tree with `tree_sitter::InputEdit`, then parses with that tree. If syntax is incomplete, the analyser uses the contiguous prefix before the first erroneous top-level construct. It does not join valid fragments across an error. Source interpretation is recomputed from that prefix with fresh bindings. Embedded payloads become available when their enclosing source can be decoded.

A syntactically complete prefix is still provisional. For example, `open('a','w').write('x')` can later become `open('a','w').write('x') if False else None`. The later revision must remove the first prediction. `syntaxComplete` is a parser property, not an execution or stream-completion signal.

Pending operation IDs are local to `(callId, revision)`. Ignore older revisions. Do not accumulate them as an event history. Final and prepared views replace the last pending report; operation IDs in completed file observations refer to that final report. Replayed calls need a new host attempt ID. The host must clear a pending view if generation is cancelled before `finish`.

The source limit is 1 MiB. Exceeding it aborts the preview, and `finish` then returns an error. This error must not prevent the host from handling the original tool call under its normal rules. It only disables this activity preview. Coalesce token fragments before analysis and publication. Incremental parsing does not make the semantic pass or full JSON view incremental; byte-at-a-time updates cost more CPU and bandwidth as the source grows.

## Capture

`finish` consumes the stream. `prepare` consumes the final call. `complete` consumes the prepared call. The types prevent a second completion on the same value and keep capture out of the pending phase.

Capture immediately before the actual dispatch, after any approval delay or queue wait. Capture the rewritten final tool arguments if hooks change them. Observe at the execution host, with the correct environment, cwd and permission scope. An outer REPL preview is not a reason to snapshot the UI machine. A nested runtime call with concrete arguments can provide better targets than the outer source.

`SnapshotProvider` lets the host supply its filesystem implementation. `LocalFiles` is an opt-in implementation for an existing local root. It accepts absolute paths under that canonical root, rejects symlinks and `..` components, and reads only UTF-8 regular files without NUL bytes. On Unix it uses no-follow and nonblocking open flags for the leaf. Use the canonical root path for cwd; platform aliases such as `/var` and `/private/var` are not silently treated as equal.

Capture is limited to 256 paths, 128 KiB per file and 1 MiB of text per phase. Tool output is limited to an 8 KiB UTF-8 excerpt. Unavailable snapshots include a reason. Original contents are held in memory and excluded from the prepared JSON. Diffs and tool output can still contain private data; the host controls their display, retention and telemetry.

The adapter does not provide an atomic snapshot, a filesystem sandbox or a wall-clock I/O deadline. Directory components can change between checking and opening. Capture is sequential, so files are not observed at one instant. A production host should use its existing filesystem capability and async I/O layer, enforce deadlines and permissions there, and isolate or serialize writers when stronger attribution is needed.

Known read targets are included because their original contents can support later comparisons. Directory listings and process launches are not treated as file-content targets. Dynamic targets appear in `unobservedOperationIds` when an emitted file operation cannot be resolved. Unsupported code can also contain unrecognised operations; these remain in the report's unresolved regions. Neither list is an inventory of all runtime effects.

The host can add known paths before execution, including files affected inside unsupported code. Such paths have no fabricated operation IDs. A path discovered only after execution has no valid original snapshot. A future filesystem journal or isolated overlay can supply that missing baseline. A post-call Git diff alone cannot recover it.

## Results

Use the actual structured terminal result. `ExecutionResult::from_exec_command_json` accepts the `exit_code` and `output` fields of a terminal command result. It rejects a live `session_id` or missing exit code. It does not search output text for a number or a success phrase. Other tool formats need their own adapter. Failed starts and cancellation use explicit status variants.

Do not call `complete` on a yielded session. Wait for the process result and for relevant writers to stop. A cancelled parent can leave child processes alive; a host that cannot establish quiescence must not present its comparison as a final state. The prototype does not own process supervision.

| Execution | File endpoints | Valid statement |
|---|---|---|
| Exit 1 | Different | The tool failed; this file changed during the observation interval |
| Exit 0 | Equal | The tool exited successfully; this file has no net content change |
| Exit 1 | Equal | The tool failed; this file has no net content change |
| Failed to start | Different | A change was observed; it is not evidence that the tool ran |
| Any status | A snapshot unavailable | File result is unknown; no diff is supplied |

`created`, `deleted`, `modified` and `unchanged` compare endpoint contents. Empty-file creation is `created` even though its text diff can be empty. A read of a missing file can fail while both snapshots remain `missing`. Equal contents do not prove that a write was skipped: a program can write the same data or change it and restore it. Metadata-only changes are outside this text comparison.

Regex substitutions, backreferences, callbacks and slice edits use the same evidence path: original contents plus actual final contents. Rust does not need to reproduce Python `re` or JavaScript replacement rules. Static transform labels explain the intent; the observed diff explains the net result. The prototype cannot give an exact pending diff before that code executes.

Concurrent writers remain a limit. A changed target with a matching predicted write is useful evidence, but it is not proof of authorship. The JSON therefore says `observationBasis: "captureInterval"` and keeps source operations as `staticIntent`. Rename/delete intent, aliases, persistent REPL state, arbitrary helper functions and writes to unknown paths still need more work.

## Runnable evidence

`cargo run --example lifecycle` emits five JSON lines from a real, fixed Python fixture. It edits `first.txt` with a regex, fails when reading `missing.txt`, and never reaches the write to `second.txt`. The stored [sample](../examples/lifecycle.output.jsonl) replaces only its temporary root with `/demo/workspace` for readability.

The runtime tests also execute a JavaScript regex callback. The Rust analyser does not evaluate that callback. The host comparison still returns the correct changed text.
