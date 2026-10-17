"""Portable panel-only adapter to unchanged frozen semantic checker functions.

Reads saved JSON only. No subprocess, represented-source execution, network,
historical corpus lookup, or acquisition of host files named by fixtures.
"""
import argparse
import hashlib
import json
from pathlib import Path
import sys

HERE = Path(__file__).resolve().parent
PACKAGE = HERE.parent
sys.dont_write_bytecode = True
sys.path.insert(0, str(HERE / "frozen_python24"))
import artifacts
import score


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--responses", type=Path, required=True)
    parser.add_argument("--candidate-label", default="unattested candidate")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    manifest = json.loads((PACKAGE / "FIXTURE-MANIFEST.json").read_bytes())
    for pin in manifest["files"]:
        raw = score.read(PACKAGE / pin["path"])
        score.need(len(raw) == pin["bytes"] and hashlib.sha256(raw).hexdigest() == pin["sha256"], "fixture/helper pin mismatch")
    requests = score.rows(score.read(PACKAGE / "fixtures/python24/requests.jsonl"))
    proofs = score.parse(score.read(PACKAGE / "fixtures/python24/source-proofs.json"))
    specs = {item["id"]: item for item in proofs["specs"]}
    score.need(len(requests) == len(specs) == 24, "fixed Python24 denominator")
    raw = score.read(args.responses)
    responses = score.rows(raw)
    wrapped = [item if set(item) == {"elapsedNs", "result"} else {"result": item} for item in responses]
    report = score.summarize(requests, specs, wrapped, artifacts, "panel")
    report.update({"schema": "jimmyCloudPython24SavedScore/v1", "candidateLabel": args.candidate_label,
                   "responsesSHA256": hashlib.sha256(raw).hexdigest(), "freshHoldout": False,
                   "representedSourceExecuted": False, "logicalResultsArePhysicalEdits": False,
                   "limit": "Panel-only saved-data semantic checks. Candidate build/process identity is not attested by this adapter. Historical full-suite CLI/pins are not used."})
    with args.output.open("x") as stream:
        json.dump(report, stream, indent=2, allow_nan=False)
        stream.write("\n")
    errors = sum(bool(item.get("errors")) or any(effect.get("errors") for effect in item.get("effects", [])) for item in report["results"])
    print(json.dumps({"cases": len(report["results"]), "caseErrors": errors, "totals": report["totals"]}))
    raise SystemExit(bool(errors))


if __name__ == "__main__":
    main()
