"""Compare complete native syntax trees on inert sources; no Python execution."""

import hashlib
import json
import pathlib
import subprocess

ROOT = pathlib.Path(__file__).resolve().parent
CASES = json.loads((ROOT / "native-equivalence-cases.json").read_text())


def parse(variant, source, suffix, replacement=None):
    input_file = ROOT / "probe-input.py"
    input_file.write_text(source)
    dump_file = ROOT / (suffix + ".cst")
    command = ["/usr/bin/nice", "-n", "10", str(ROOT / ("native-" + variant)),
               str(input_file), "1", str(dump_file)]
    if replacement is not None:
        edit_file = ROOT / "probe-edit.py"
        edit_file.write_text(replacement)
        command.append(str(edit_file))
    result = subprocess.run(command, capture_output=True, text=True, check=True, timeout=30)
    return json.loads(result.stdout), dump_file.read_bytes()


records = []
for case in CASES:
    before, old_tree = parse("before", case["source"], "before")
    after, new_tree = parse("indent", case["source"], "after")
    if old_tree != new_tree:
        raise RuntimeError("fresh CST mismatch: " + case["id"])
    edits = ["# inserted inert comment\n" + case["source"],
             case["source"] + '\nopen("late", "w").write("x")\n',
             case["source"].replace("    ", "", 1)]
    edit_records = []
    for replacement in edits:
        old, old_incremental = parse("before", case["source"], "before-edit", replacement)
        new, new_incremental = parse("indent", case["source"], "after-edit", replacement)
        old_fresh, old_fresh_tree = parse("before", replacement, "before-fresh")
        new_fresh, new_fresh_tree = parse("indent", replacement, "after-fresh")
        if old_incremental != new_incremental or old_fresh_tree != new_fresh_tree:
            raise RuntimeError("edited CST mismatch: " + case["id"])
        if new_incremental != new_fresh_tree:
            raise RuntimeError("incremental/fresh mismatch: " + case["id"])
        edit_records.append({"sourceChanged": replacement != case["source"],
                             "sourceSha256": hashlib.sha256(replacement.encode()).hexdigest(),
                             "cstSha256": hashlib.sha256(new_incremental).hexdigest(),
                             "nodes": new["nodes"], "syntaxError": new["syntaxError"]})
    records.append({"id": case["id"], "sourceSha256": hashlib.sha256(case["source"].encode()).hexdigest(),
                    "cstSha256": hashlib.sha256(new_tree).hexdigest(),
                    "nodes": after["nodes"], "syntaxError": after["syntaxError"],
                    "incrementalEdits": edit_records})

(ROOT / "native-equivalence-results.json").write_text(json.dumps({
    "freshCases": len(records), "incrementalEdits": 3 * len(records),
    "changedInputEdits": sum(edit["sourceChanged"] for record in records for edit in record["incrementalEdits"]),
    "noOpEdits": sum(not edit["sourceChanged"] for record in records for edit in record["incrementalEdits"]),
    "comparison": "exact full CST bytes: kinds, fields, all children/comments, byte/point spans, named/extra/error/missing flags; candidate/upstream and incremental/fresh",
    "records": records,
}, indent=2) + "\n")
print(f"PASS {len(records)} fresh trees and {3 * len(records)} incremental edits")
