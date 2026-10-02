# Native scanner experiment

These tools parse inert Python sources through the pinned native tree-sitter C
runtime and grammar. They never run Python or access a source program's targets.
The runtime is tree-sitter 0.25.10; the original grammar is tree-sitter-python
0.25.0, upstream commit `293fdc02038ee2bf0e2e206711b69c90ac0d413f`.

`native-equivalence-cases.json` retains 117 source cases from that upstream
commit's `test/corpus` and 96 generated comment/indentation/Unicode/CRLF/error
cases. The upstream sources retain the full MIT notice in
`LICENSE.tree-sitter-python`. Fresh parses and three incremental edits per case
compare exact complete CST bytes: node kinds/fields, all children and comments,
byte/point spans, named/extra/error/missing flags. The result has 213 fresh cases
and 639 incremental checks: 526 changed-input edits and 113 no-op edits where
the indentation recipe finds no four-space sequence. Each incremental result
also equals a fresh parse of its replacement source. FNV signatures in timed
scaling rows are only diagnostic checks;
the differential gate compares the entire serialized CST byte sequence.

Reproduce on macOS with the two original packages in your Cargo registry and the
patched grammar checked into this fork. Supply actual package directories; the
build driver neither discovers nor downloads arbitrary packages.

```sh
python3 build-native.py /path/tree-sitter-0.25.10 /path/tree-sitter-python-0.25.0 /path/fork/third_party/tree-sitter-python /tmp/native-replay
cp native-equivalence.py native-equivalence-cases.json measure-native.py /tmp/native-replay/
python3 /tmp/native-replay/native-equivalence.py
python3 /tmp/native-replay/measure-native.py
```

Builds and measurements are serial at nice 10. Native compilation uses clang
`-O3 -std=c11`; this is a separate native diagnostic, not the Rust release
consumer. `native-build-receipt.json` records exact commands, source and binary
hashes, and compiler version. `native-scaling.jsonl` retains every measured
nanosecond sample, warmup/count, source hash, node count and summary.

Timing includes the reused parser's native parse plus Tree destruction. Source
generation, file/process startup, parser setup, full CST inspection, output and
statistics are excluded. Incremental checks are correctness evidence, not
incremental performance results. At least one warmup precedes all native
samples. Cases at 1024 or more comments use three samples; p95 is then the maximum.

The lower-indented-comment family intentionally retains original lookahead where
DEDENT can still depend on following code. It is a negative control and records
remaining cost rather than implying a universal deadline. Complete Rust
parse/emission/consumer measurements are separately recorded one directory up.
