# Conditional edit previews

`Analyzer::forecast_edits` accepts bounded, complete source and returns
individually conditional mutation recipes. It may retain an understood operation
whose dependencies are unaffected by an unresolved scalar or a later barrier.
Every retained operation keeps its source evidence, captured values, read
identities, predecessor conditions and runtime assumptions. Unresolved regions
remain visible. An analyzed disposition or a nonempty result is not evidence
that the whole script is understood.

This API is separate from `analyze_complete`, which rejects the entire effect
interpretation when its contract is unsupported, and from
`describe_complete`, which requires a complete source-description grammar but
does not attest execution. The older partial extraction API remains available
for diagnostic comparisons. None of these APIs grants execution permission.

## Python and Node semantics

The Python lane models a closed set of standard-runtime operations. Supported
text recipes include immutable reads, concatenation, literal replacement,
code-point slices with positive unit step, substring indices, bounded split
items, explicit-character stripping and lazy startswith/endswith conditionals.
Some bounded literal loops and closed path bindings are supported. Aliases of
captured immutable values retain their earlier identity after later assignment.
Selected missing substring indices produce a typed unavailable result; an
unselected conditional arm is not evaluated by the materializer.

The Node CommonJS lane is distinct from CodeMode TypeScript. It recognizes
canonical synchronous UTF-8 `fs` reads/writes, direct `const` destructuring of
the supported `fs` functions, primitive-string concatenation, and literal
first-match or all-match replacement. All-match replacement requires a nonempty
needle; replacement-dollar substitutions are excluded. Node newlines are
preserved. Python code-point slicing is not reused as a JavaScript UTF-16 model.

Import spelling does not prove module identity. Standard builtins, module and
method identity, startup context, cwd, I/O behavior and the absence of conflicting
mutation remain explicit premises where the source cannot establish them.
General helpers, dynamic paths, arbitrary control flow, persistent interpreter
state and unsupported scope behavior can require a barrier or whole-source
refusal. A later operation may still depend on normal completion of an earlier
expression even when its output bytes do not depend on that expression's value.

## Supplied-snapshot materialization

`materialize_snapshot_preview` is pure over supplied data. It does not open a
target file or execute the source. Its caller supplies a `SnapshotPreviewInput`
with the call namespace, target preimage, read snapshots,
source/read identities, mutation epochs, exact path/cwd spellings, encoding and
newline profile, and selects the operation with a separate ordinal argument.
A missing preimage and an absent file are different assertions.

The library checks those bindings against the selected recipe. It does not
attest the caller or prove that bytes are current. A source-local epoch counts
modeled mutations; it is not a filesystem timestamp. Different path spellings
are not resolved into one object. The caller must establish the operation-point
snapshot and runtime premises before treating an output as an applicable
preview.

A successful result contains exact supplied-before and conditional-after bytes,
a bounded unified diff and, when bytes differ, a UTF-8 replacement range. Its basis remains
`suppliedSnapshotConditional` and its scope remains `operationPoint`.
Conflicting bindings, missing snapshots, modeled exceptions and exceeded limits
return errors without a partial exact artifact. The result does not establish
whether execution reached the operation, whether a write succeeded, or whether
later code changed the same file. An unchanged endpoint also does not mean that
no write operation occurred.

The default materializer limits are 1 MiB of metadata-inclusive input, 1 MiB
intermediate/output text, 32 MiB cumulative charged work and 4 MiB diff. The
submitted-source presentation boundary uses a 32 KiB aggregate source limit.
These limits bound admitted data/work; they are not claims about aggregate
resident memory or a hard wall-clock limit.

## Native presentation boundary

The core integration admits one presentation job after final tool-binding
transformation and awaits it before creating the execution runtime. Scheduling,
setup, conditional forecasting and complete-source fallback consume the same
existing deadline. A validated, presentable nonempty forecast can enrich the
source row. Unavailable, empty or disclosure-rejected forecasts retain the
source-description fallback. An empty forecast is not displayed as "no edits."

Protocol conversion and compact/expanded TUI cells carry the recipes, source
evidence, assumptions and uncertainty. They do not create FileChange/TurnDiff
events or an Applied/Succeeded result. Runtime source, enabled tool definitions,
approval inputs, Guardian decisions and sandbox permissions are unchanged.

There is no automatic trusted native snapshot supplier in this implementation.
The native cell therefore presents conditional operations and recipes; it does
not yet deliver materialized filesystem diffs. The standalone JSONL snapshot
example exercises the pure supplied-snapshot API. It is not evidence of live
native snapshot capture or rendering.

Protocol enums and generated TypeScript/JSON schemas require coordinated
same-version activation. There is no backward negotiation with older clients
that reject unknown enum variants. Python SDK generation and client validation
remain separate integration work.

## Reproducible inert boundaries

The `source_boundary` example accepts inert JSONL and supports
`--selective-edits` and `--selective-benchmark`. The
`snapshot_preview_boundary` example accepts inert source plus explicitly supplied
synthetic snapshots. Its `--benchmark` mode times decode, construction and,
for accepted inputs, analysis, binding, materialization and serialization.
Neither example executes source or reads target paths.

Focused integration tests live in `tests/selective_python_*`,
`tests/selective_javascript_*` and `tests/selective_snapshot_preview*`.
Protocol serialization/schema parity and native conversion/presentation have
separate tests. Passing a parser, a source-description test or a schema-generation
fixture comparison alone does not establish effect or preview correctness.
