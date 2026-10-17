# Current-host integration boundary

The `codex_protocol_demo` example exercises actual Codex command lifecycle types
and legacy conversion, with independent endpoint observations. It is a local,
two-phase integration aid. It does not install a core dispatcher, app-server
transport, TUI renderer, approval hook or controlled Python executor.

## Runtime trust is unavailable on the ordinary command path

Official Codex main `cc7ba33601286a751477832b01de7b6f8de0dd9c` still launches
configured shell commands through the ordinary environment, hook, policy,
approval and sandbox path. That path has no canonical Python module preload or
verified child-readiness/source-release channel. Shell snapshots and configured
runtime resolution remain observable behavior. A fresh process, familiar import,
`-I`, sandbox or successful exit does not establish `PreloadedTrustedPathlib`.

Consequently this example always supplies `PersistentOrUnknown` to the guarded
command bridge. The complete command is available before dispatch, but unknown
runtime provenance yields explicit fallback and zero additional rows/diffs.
There is no model-controlled trust field. The original argv and cwd are retained;
parser fallback does not deny execution or grant authority.

Do not silently rewrite ordinary Python invocations to the earlier controlled
bootstrap. That would change interpreter selection, startup, imports, globals,
stdin/process lifecycle and possibly visible initialization effects. A positive
trusted-profile integration needs a separately selected executor capability,
including the exact user source in existing approval/Guardian review, a verified
ready child inside the selected sandbox, and attempt-local source release.

## Example protocol

Read exactly one bounded JSON launch line:

```json
{"callId":"demo","command":["/bin/bash","-c","complete authored command"],"cwd":"/absolute/disposable/directory","startedAtMs":1000}
```

The example emits and flushes a `prepared` line containing the actual
`ItemStartedEvent`, upstream `HasLegacyEvent` conversion, untouched command
item, strict fallback and zero rows. It captures only the host-selected
`settings.txt`. Its empty observation stream is not command analysis; the
separate guarded bridge handles the complete source. No captured command is
executed by this Rust example.

A trusted local controller checks that frame before launching the exact argv/cwd.
After the child stops, it sends one actual completion and closes stdin:

```json
{"exitCode":0,"output":"actual output","durationMicros":1234,"completedAtMs":1002}
```

The example then emits the actual `ItemCompletedEvent` and legacy completion
conversion, plus separately labeled `CaptureInterval` observations. Failed
processes can still have changed files; zero exits and success-like output need
not mean a file changed. Observed diffs have `HostSnapshotsOnly` attribution.
They are never manufactured `FileChangeItem` events or predicted patch bytes.
Input framing, byte caps, deadlines and consumed completion prevent reuse in this
demo. The controller remains responsible for real execution and terminal truth.

## Two-case evidence

The focused harness uses the same fixed Python source twice:

```python
from pathlib import Path
p = Path('settings.txt')
p.write_text(p.read_text().replace('mode=old', 'mode=new'))
```

With standard pathlib it changes one line. With an authored harmless cwd module
that replaces `Path`, it prints an edit-success message, returns normally, and
changes nothing. Both correctly retain unknown-context fallback. Before/after
captures distinguish the outcomes without guessing from source or output.
PR1's partial direct-source interpreter returns read/edit intent rows while
also reporting unanalyzed import initialization; that partial contract must not
be presented as the stronger whole-script effect guarantee.

These are selected synthetic examples, not field-coverage estimates. The harness
uses disposable macOS sandboxes, denied network, cwd-only writes, exact executable
allowlists, no child forks and bounded resources. Separate authored probes check
network and outside-write denial; they do not validate Codex Guardian or approval.

## Remaining upstream work

The relevant command types and conversions in this PR match current main, but
whole protocol files and host execution code differ. A production feature still
needs final post-hook/attempt identity, a truthful capability producer, bounded
async-host waiting/cancellation, transport/redaction/replay, approval-created
start deduplication, native rendering and host lifecycle tests. Known-file
snapshots require authorized host paths and a defined capture interval; they are
not complete filesystem traces or writer attribution.

The narrow local contribution here is the consumer example and evidence. It
changes no execution or security policy, dependency, production protocol or UI.
Do not advertise an always-fallback bridge as a shipped Python edit preview.
