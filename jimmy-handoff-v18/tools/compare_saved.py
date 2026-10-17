"""Compare saved inert analyzer behavior. This is NOT an accuracy scorer."""
import argparse
import json
from pathlib import Path


def load(path):
    data = path.read_bytes()
    if len(data) > 32 * 1024 * 1024:
        raise ValueError("capture exceeds 32 MiB")
    rows = [json.loads(line) for line in data.splitlines()]
    return [item["result"] if set(item) == {"elapsedNs", "result"} else item for item in rows]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--expected", type=Path, required=True)
    parser.add_argument("--responses", type=Path, required=True)
    args = parser.parse_args()
    expected, actual = load(args.expected), load(args.responses)
    ids = [item["id"] for item in expected]
    if len(set(ids)) != len(ids) or len(actual) != len(expected):
        raise ValueError("duplicate reference ID or incorrect response row count")
    changed = [cid for cid, left, right in zip(ids, expected, actual, strict=True) if left != right]
    print(json.dumps({"rows": len(expected), "unchanged": len(expected) - len(changed),
                      "changedIds": changed, "metric": "saved behavior equality; not correctness or coverage"}))
    raise SystemExit(bool(changed))


if __name__ == "__main__":
    main()
