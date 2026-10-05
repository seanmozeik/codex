# Codex adapter

`analyze_codex_command` borrows Codex's actual `CommandExecutionItem` and returns
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

## Host integration

The crate is not wired into core dispatch, app-server events, thread history or
TUI rendering. Its policy API is tested against mock dispatch; production
pre-execution enforcement remains separate. A host integration must:

- Publish the [versioned contract](output-contract.md) with partial/unsupported
  status and apply secret/path disclosure rules before transport.
- Coalesce source deltas, replace stale previews and analyze the final arguments
  after hook rewrites. Match nested predictions to actual child calls separately.
- Preserve item lifecycle/history and reconcile predictions with terminal
  results. Runtime status does not turn static intent into observed changes.
- Use the selected executor's filesystem and permissions for optional capture;
  enforce native parsing/I/O deadlines and process supervision at the host.

See [lifecycle](lifecycle.md) for revision and observation semantics.
