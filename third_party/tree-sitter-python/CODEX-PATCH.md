# Python scanner lookahead optimization

This is the MIT-licensed crates.io `tree-sitter-python` 0.25.0 package, from
upstream commit `293fdc02038ee2bf0e2e206711b69c90ac0d413f`. The complete license,
original Rust bindings/build script and generated C parser are retained. The
package's standalone lockfile and Cargo registry cache metadata are omitted.
`source-manifest.json` records original and selected source hashes.

Only `src/scanner.c` changes. `scanner-lookahead.patch` is the complete patch.
The scanner previously rescanned the remaining comment suffix even when no
external token could be returned. After recording the first comment indentation,
the guard returns false when INDENT/NEWLINE are disabled and that indentation
rules out DEDENT. Encountering a comment already rules out STRING_START. Error
recovery requires INDENT and therefore keeps the original path. The native lexer
then consumes the original comment; no source, comment or syntax validation is
removed. The first comment indentation matters when later comments differ.

The grammar definition `grammar.js` is retained as upstream provenance; it is
not evaluated by Cargo or the analyzer. Parsing uses compiled C through the
unchanged Rust FFI bindings. No Tripwire TypeScript implementation or JavaScript
runtime is added.

The full-CST and incremental differential evidence, consumer regressions and
measured scaling are documented in the code-activity performance artifacts.
The unchanged scanner paths can still have expensive lookahead; this targeted
optimization does not establish a hard native parsing deadline.
