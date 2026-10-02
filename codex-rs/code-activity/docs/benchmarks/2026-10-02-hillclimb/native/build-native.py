"""Build serial native probes from caller-supplied, pinned public sources.

Usage: build-native.py TREE_SITTER_RUNTIME ORIGINAL_PYTHON PATCHED_PYTHON OUTPUT
The C grammar only parses inert input; no source program is executed.
"""

import hashlib
import json
import pathlib
import subprocess
import sys


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    runtime, original, patched, output = map(lambda value: pathlib.Path(value).resolve(), sys.argv[1:])
    output.mkdir(parents=True, exist_ok=True)
    probe = pathlib.Path(__file__).with_name("native-probe.c")
    records = []
    for name, grammar in [("before", original), ("indent", patched)]:
        command = ["/usr/bin/nice", "-n", "10", "/usr/bin/clang", "-O3", "-std=c11",
                   "-D_DARWIN_C_SOURCE", "-D_POSIX_C_SOURCE=200112L",
                   "-I" + str(runtime / "include"), "-I" + str(runtime / "src"),
                   "-I" + str(grammar / "src"), str(runtime / "src/lib.c"),
                   str(grammar / "src/parser.c"), str(grammar / "src/scanner.c"),
                   str(probe), "-o", str(output / ("native-" + name))]
        subprocess.run(command, check=True)
        files = [probe, *runtime.glob("src/**/*"), *runtime.glob("include/**/*"),
                 grammar / "src/parser.c", grammar / "src/scanner.c",
                 *grammar.glob("src/tree_sitter/*")]
        records.append({"variant": name, "command": command,
                        "binarySha256": digest(output / ("native-" + name)),
                        "sourceSha256": {str(path): digest(path) for path in files if path.is_file()}})
    (output / "native-build-receipt.json").write_text(json.dumps({
        "clang": subprocess.check_output(["/usr/bin/clang", "--version"], text=True),
        "runtime": "tree-sitter 0.25.10", "grammar": "tree-sitter-python 0.25.0",
        "probe": "same C driver; reused parser; parse and tree destruction; exact CST inspection excluded",
        "records": records,
    }, indent=2) + "\n")


if __name__ == "__main__":
    main()
