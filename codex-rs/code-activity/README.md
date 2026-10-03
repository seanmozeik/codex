# Code activity

This crate provides the analysis building blocks for showing and reviewing
code-tool activity in Codex. It turns Python, JavaScript/TypeScript and shell
source into structured file and process intent without executing it. It
recognizes reads, writes, appends, edits, deletion, listings and process launches,
including literal nested
tool and interpreter calls. Local functions, closures and supported callbacks
retain their source locations and repeated effects.

`ActivityContract` gives clients a versioned envelope and typed activity rows.
Every record is static intent, with source spans and nested-source provenance;
unknown targets and unsupported semantics remain explicit. Streaming previews
replace earlier revisions rather than accumulating stale predictions.

The Codex adapter borrows the real `CommandExecutionItem`, preserves its existing
parsed actions, cwd URI and execution status, and adds a separate analysis view.
The crate does not yet publish app-server events or render a live Codex UI.

A whole-script policy API inspects the combined activity before mock dispatch.
It flags selected literal sensitive-file access, root deletion and force-push
operations, including recognized operations inside nested scripts. Tests show
known hits blocking the entire mock dispatch, even after benign operations, and
uncertain input escalating without dispatch. The API binds the decision to the
reviewed source and cannot grant permission. Real Codex execution enforcement
remains a host integration task.

## Use

From `codex-rs`:

```sh
cargo run -p codex-code-activity --example codex_item --locked
cargo run -p codex-code-activity --example consumer --locked
cargo run -p codex-code-activity --bin codex-code-activity --locked < code-activity/examples/python-edit.jsonl
cargo nextest run -p codex-code-activity --all-targets --locked --run-ignored all --retries 0
cargo test -p codex-code-activity --doc --locked
```

The JSONL CLI and command-item example analyze inert source. The lifecycle example
and two opt-in runtime tests execute only fixed fixtures in temporary directories.

- [Codex adapter](docs/codex-integration.md): supported carriers and host integration.
- [Output contract](docs/output-contract.md): records, identity and consumer rules.
- [Lifecycle](docs/lifecycle.md): streaming, host capture and terminal results.
- [Benchmarking](docs/benchmarking.md): reusable release and streaming harnesses.

## Limits

Coverage is always partial or opaque. A predicted write is not evidence that it
ran; an empty report does not prove the absence of activity or grant permission.
The host owns execution, permissions, redaction and disclosure.

Source is limited to 1 MiB, operations and uncertainty records to 256 each, nested
sources to 64, embedded depth to 4, syntax depth to 64 and semantic visits to
50,000 per source. Abstract values, bindings, functions and literal callback
iteration are also bounded. These limits do not impose a native parsing deadline.

Dynamic evaluation, arbitrary module initialization/user code, persistent REPL
state, general shared-container mutation and full shell semantics are unmodeled.
Classes/methods, Python async/generator/global/nonlocal semantics, JavaScript var
hoisting and most callback families require explicit uncertainty.

Activity interpretation and consumers are Rust, with native tree-sitter syntax
parsing. See [parser dependencies](docs/dependencies.md) for the immutable
Python scanner patch and build requirements.

Selected parsing scenarios and mock policy rules are adapted from Tripwire.
[NOTICE](NOTICE) retains the required MIT attribution; this crate inherits Codex's
Apache-2.0 license metadata. The policy example never authorizes execution.
