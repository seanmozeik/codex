# Isolated Bash grammar for source previews

This internal crate pins `tree-sitter-bash` 0.25.1 and corrects one external-scanner
condition. Complete source descriptions and conditional edit forecasts use this
grammar. Ordinary analysis, whole-script analysis, and streaming keep their
existing `tree-sitter-bash` dependency and parser.

## Provenance

- Upstream: <https://github.com/tree-sitter/tree-sitter-bash>
- Registry release: `tree-sitter-bash` 0.25.1.
- Registry archive SHA-256:
  `9e5ec769279cc91b561d3df0d8a5deb26b0ad40d183127f409494d6d8fc53062`.
- Upstream commit from the registry archive's `.cargo_vcs_info.json`:
  `a06c2e4415e9bc0346c6b86d401879ffb44058f7`.
- License: upstream MIT license, retained unchanged in `LICENSE`.
- `UPSTREAM.sha256` records the original copied-file hashes. `grammar.js`,
  `tree-sitter.json`, generated `src/parser.c`, `src/grammar.json`,
  `src/node-types.json`, and the three C headers are copied unchanged. No parser
  regeneration was needed.

The official [release page](https://github.com/tree-sitter/tree-sitter-bash/releases)
still lists 0.25.1 as latest as checked on 2026-10-14. The official
[master scanner](https://github.com/tree-sitter/tree-sitter-bash/blob/master/src/scanner.c)
still contains the leading-zero condition. Upstream
[PR 156](https://github.com/tree-sitter/tree-sitter-bash/pull/156) established
descriptor support for heredocs; the pinned grammar already has that production.
The correction here addresses scanning rather than adding grammar syntax.

## Scanner delta

The only changed upstream line is in `src/scanner.c`, in the special-variable
pre-scan:

```diff
-             lexer->lookahead == '0' || lexer->lookahead == '_')) {
+             (lexer->lookahead == '0' && !valid_symbols[FILE_DESCRIPTOR]) || lexer->lookahead == '_')) {
```

When a descriptor token is valid, zero now follows the existing digit scanner,
as the other digits do. That scanner emits a descriptor only when the digits
are immediately followed by `<` or `>`. A separated, quoted, or escaped zero is
still an argument. When a descriptor is not valid, positional-parameter scanning
retains the original special-zero branch.

Before the correction, `0<<` entered the special-zero branch, marked the token
end before zero, and then entered the heredoc-arrow branch. This prevents an
ordinary descriptor token and leaves erroneous heredoc state/token boundaries.
The correction parses the submitted bytes directly; it does not edit source,
remove error nodes, or bypass complete-source validation.

## Link isolation and cost

`build.rs` compiles the copied generated C and corrected scanner with the six
public `tree_sitter_bash*` symbols renamed to `codex_tree_sitter_bash*` using C
preprocessor definitions. Both definitions and their references are renamed
together. The Rust wrapper exposes only the standard `LanguageFn` constant. No
Tree-sitter internal structure is copied or modified at runtime.

The vendored C and headers occupy 9,976,933 bytes, including a 9,912,254-byte
generated parser. Linking both grammars retains a second grammar table and
scanner; each `Analyzer` also owns a separate preview parser. The final binary
size and initialization-time increments must be measured in the normal build;
source-only inspection does not establish those costs. Keeping the two grammars
separate prevents this preview correction from changing approval-related parsing.

## Updating

Replace the upstream files together from a pinned, licensed release and refresh
`UPSTREAM.sha256`. Keep this scanner delta only until an upstream release fixes
the same descriptor boundary. Preserve symbol isolation while preview and strict
grammars differ. Run the raw-CST, heredoc ownership, numeric descriptor, selective
carrier, and strict shell regressions before changing either parser selection.
Do not use a workspace-wide dependency override for this correction.
