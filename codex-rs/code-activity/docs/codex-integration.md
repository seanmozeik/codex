# Codex adapter

The historical `analyze_codex_command` borrows Codex's actual `CommandExecutionItem` and returns
an additive static-analysis view. Existing parsed commands, argv, aggregate
output, cwd URI, process identity, status and exit code remain on the original
item. Predicted activity is independent of command success or failure.

The adapter uses Codex's existing shell extractors on original argv. Sh, Bash and
Zsh carriers receive best-effort Bash grammar analysis. PowerShell, direct
Python/Node argv and unsupported or oversized input return explicit unsupported
results. The standalone analyzer also accepts Python and TypeScript source.

The parser starts with unresolved cwd. The executor `PathUri` stays on the
borrowed item; a foreign URI is not converted into a host-local filesystem path.
Literal source cwd changes describe conditional intent. `LocalFiles` is an
opt-in standalone capture provider, not a Codex executor adapter.

Run the command-item example from `codex-rs`:

```sh
cargo run -p codex-code-activity --example codex_item --locked
```

## Opt-in complete-source bridge

`analyze_complete_codex_command` is a separate strict prototype. It accepts the
actual `CommandExecutionItem`, explicit `EnvironmentAssumptions`, and
`AnalysisLimits`, then analyzes its final complete source with `analyze_complete`.
It does not change the historical adapter or `ActivityContract` v1 behavior.

The result retains the exact original item at `item()`, including existing parsed
commands, source argv, status, output, executor cwd URI, and process metadata.
`ui_rows()` exposes only additional static-intent rows. Every unsupported carrier
or strict fallback yields zero added rows. The host must preserve its ordinary
command presentation in all cases; it must not replace existing rows with this
possibly empty list. `source_activity()` retains the explicit carrier reason or
the strict disposition and diagnostics. No snapshots are supplied or observed,
so this bridge cannot produce diffs or execution receipts.

Only wrappers recognized by Codex's existing extractor with executable basename
`bash` and exactly `-c` or `-lc` plus a script are candidates. Sh, Zsh, PowerShell,
direct Python/Node argv, malformed argv, and oversized source abstain. The byte
limit is checked before copying the script. Fresh-process assumptions also
require ordinary executable identity and absent startup effects; a `bash` name
or a new process does not establish those facts. `PersistentOrUnknown` remains
the appropriate choice when the host cannot establish the assumptions.

Call this bridge only after transport assembly and hook rewrites have finalized
the exact argv on the item. An `InProgress` item can have complete source: that
status refers to the process, not incomplete transport input. Items marked
`UnifiedExecInteraction`, or carrying any `interaction_input` including empty
input, abstain because stdin fragments are not complete initial programs. Do not
re-analyze the original process argv as if it described a later stdin write.

Generic `functions.exec` source is JavaScript orchestration, not a command item.
Do not wrap that source in artificial Bash argv or treat all tool-call payloads
as command scripts. An actual nested `exec_command` invocation can produce a
complete `CommandExecutionItem`; analyze that child item independently. This
bridge does not infer whether another orchestration call actually launched it.

For a finalized item already obtained from the host:

```rust,ignore
let view = analyze_complete_codex_command(
    &mut analyzer,
    &item,
    established_environment_assumptions,
    AnalysisLimits::default(),
);
// Retain the existing command presentation from view.item().
// Only view.ui_rows() may be added as static intent.
```

This remains an unwired bridge prototype. It installs no renderer, event handler,
dispatch hook, permission decision, sandbox change, or Guardian bypass. The host
continues its existing execution and approval path regardless of the result.

## Host integration

The crate is not wired into core dispatch, app-server events, thread history or
TUI rendering. Its policy API is tested against mock dispatch; production
pre-execution enforcement remains separate. An initial strict host integration must:

- Buffer complete decoded input and wait for final post-hook argv before analysis.
  Emit no incremental or partial rich rows. Any unsupported uncertainty discards
  all additional rows and diffs for the whole script.
- Use the opt-in strict bridge result while preserving the ordinary command UI,
  parsed rows, lifecycle and history. Additional rows remain static intent;
  runtime status and stdout do not turn them into completed-action receipts.
- Retain carrier and strict fallback diagnostics, and apply the host's existing
  secret/path disclosure rules before transport. Match nested predictions to
  actual child command items separately, without treating generic orchestration
  as a shell-script payload.
- Use the selected executor's filesystem and permissions for optional capture;
  enforce native parsing/I/O deadlines and process supervision at the host.

The older [versioned partial contract](output-contract.md), incremental previews,
and [lifecycle experiments](lifecycle.md) remain separate experimental APIs. They
are not the initial strict presentation contract and must not be mixed into its
no-partial-emission path.
