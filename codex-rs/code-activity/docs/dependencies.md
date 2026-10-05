# Parser dependencies

The analyzer uses the workspace tree-sitter runtime and Python, TypeScript and
Bash grammars. Python uses an immutable Git dependency in `codex-rs/Cargo.toml`;
Cargo.lock and MODULE.bazel.lock resolve the same revision.

The [pinned Python dependency](https://github.com/seanmozeik/tree-sitter-python/tree/da2cb9d769837d483db688e8bf9d99093b7682e0)
is based on upstream v0.25.0, commit
`293fdc02038ee2bf0e2e206711b69c90ac0d413f`, with only seven added scanner lines at
`da2cb9d769837d483db688e8bf9d99093b7682e0`. The scanner returns early when indent
and newline tokens are disabled and the first comment's indentation excludes a
dedent. Paths that can emit indentation tokens retain upstream lookahead.

Generated C, Rust bindings, queries and upstream MIT licensing are unchanged.
The dependency compiles its checked-in parser and scanner; no generator or
runtime download is introduced. SHA-256 of the patched scanner is
`28f2a41b6d9fb4d7bd0103f82ac3024a9efcb92724947971344d37aec1055ad0`.

A first build needs the pinned Git repository, the usual registry dependencies
and the Cargo or Bazel toolchain. Cargo's `--locked --offline` works after its
source cache is populated. Bazel needs its repository and toolchain caches
populated before `--nofetch --repository_disable_download --lockfile_mode=error`.
An empty dependency cache cannot support an offline first build.
