"""Serial native scaling measurements on inert comment fixtures."""

import hashlib
import json
import math
import pathlib
import statistics
import subprocess

ROOT = pathlib.Path(__file__).resolve().parent


def summary(samples):
    ordered = sorted(samples)
    return {"p50": ordered[math.ceil(len(samples) * .50) - 1],
            "p95": ordered[math.ceil(len(samples) * .95) - 1],
            "min": ordered[0], "max": ordered[-1],
            "mean": statistics.mean(samples), "populationStddev": statistics.pstdev(samples)}


cases = []
for count in [128, 512, 1024, 2048, 4096, 8192]:
    cases.append(("module", count, 'open("input.txt").read()\n' +
                  "# padded source without extra actions\n" * count))
for count in [128, 1024, 4096]:
    cases.append(("same-indent", count, 'def f():\n    open("input.txt").read()\n' +
                  "    # same indent inert comment\n" * count + '    open("tail.txt").read()\nf()\n'))
for count in [128, 1024, 2048]:
    cases.append(("low-indent-retained", count, 'def f():\n    open("input.txt").read()\n' +
                  "# low indent inert comment\n" * count + '    open("tail.txt").read()\nf()\n'))

records = []
with (ROOT / "native-scaling.jsonl").open("w") as raw:
    for family, count, source in cases:
        path = ROOT / "native-scaling-input.py"
        path.write_text(source)
        pair = []
        for variant in ["before", "indent"]:
            iterations = 3 if count >= 1024 else 10
            command = ["/usr/bin/nice", "-n", "10", str(ROOT / ("native-" + variant)),
                       str(path), str(iterations)]
            result = subprocess.check_output(command, text=True, timeout=60)
            row = {**json.loads(result), "family": family, "commentLines": count,
                   "variant": variant, "command": command,
                   "sourceSha256": hashlib.sha256(source.encode()).hexdigest()}
            row["nanoseconds"] = summary(row["rawNanoseconds"])
            row["callsPerSecond"] = 1e9 / row["nanoseconds"]["mean"]
            raw.write(json.dumps(row) + "\n")
            raw.flush()
            pair.append(row)
        assert pair[0]["treeSignature"] == pair[1]["treeSignature"]
        assert pair[0]["nodes"] == pair[1]["nodes"]
        assert pair[0]["syntaxError"] == pair[1]["syntaxError"]
        records.append({"family": family, "commentLines": count,
                        "inputBytes": len(source.encode()),
                        "original": pair[0]["nanoseconds"], "optimized": pair[1]["nanoseconds"],
                        "medianRatio": pair[0]["nanoseconds"]["p50"] / pair[1]["nanoseconds"]["p50"],
                        "nodes": pair[1]["nodes"],
                        "sourceSha256": pair[1]["sourceSha256"]})
        print(f"{family}-{count}: {records[-1]['medianRatio']:.2f}x", flush=True)
(ROOT / "native-scaling-summary.json").write_text(json.dumps(records, indent=2) + "\n")
