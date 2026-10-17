"""Independent endpoint/range/diff checks. Source snippets are never executed."""

from __future__ import annotations

import re
from typing import Any


class ArtifactError(ValueError):
    pass


def lf_lines(data: bytes) -> list[bytes]:
    """Split on LF only, retaining CR and every other byte verbatim."""
    pieces = data.split(b"\n")
    lines = [piece + b"\n" for piece in pieces[:-1]]
    if pieces[-1]:
        lines.append(pieces[-1])
    return lines


def bytes_field(value: Any, label: str) -> bytes:
    if not isinstance(value, list) or any(type(x) is not int or not 0 <= x <= 255 for x in value):
        raise ArtifactError(f"{label}: invalid byte array")
    return bytes(value)


def utf8_boundaries(data: bytes) -> set[int]:
    try:
        text = data.decode("utf-8", "strict")
    except UnicodeDecodeError as error:
        raise ArtifactError("endpoint is not strict UTF-8") from error
    boundaries = {0}
    position = 0
    for char in text:
        position += len(char.encode("utf-8"))
        boundaries.add(position)
    return boundaries


def check_replacements(before: bytes, after: bytes, replacements: Any) -> None:
    if not isinstance(replacements, list):
        raise ArtifactError("replacements: expected array")
    before_boundaries, after_boundaries = utf8_boundaries(before), utf8_boundaries(after)
    previous_before = previous_after = 0
    reconstructed = bytearray()
    for replacement in replacements:
        try:
            left, right = replacement["before"], replacement["after"]
            a, b, c, d = left["start"], left["end"], right["start"], right["end"]
        except (KeyError, TypeError) as error:
            raise ArtifactError("replacement: missing endpoint range") from error
        if any(type(x) is not int for x in (a, b, c, d)):
            raise ArtifactError("replacement: offsets must be integers")
        if not (previous_before <= a <= b <= len(before) and previous_after <= c <= d <= len(after)):
            raise ArtifactError("replacement: unordered or out-of-bounds range")
        if a not in before_boundaries or b not in before_boundaries or c not in after_boundaries or d not in after_boundaries:
            raise ArtifactError("replacement: endpoint splits a UTF-8 code point")
        reconstructed.extend(before[previous_before:a])
        if len(reconstructed) != c:
            raise ArtifactError("replacement: after offset disagrees with reconstructed prefix")
        reconstructed.extend(after[c:d])
        previous_before, previous_after = b, d
    reconstructed.extend(before[previous_before:])
    if bytes(reconstructed) != after:
        raise ArtifactError("replacement ranges do not reconstruct after bytes")
    if before == after and replacements:
        raise ArtifactError("no-op has nonempty replacement ranges")
    if before != after and not replacements:
        raise ArtifactError("changed bytes have no replacement range")


HUNK = re.compile(rb"@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@(?:[^\n]*)\n?\Z")


def apply_unified_diff(before: bytes, diff: str) -> bytes:
    """Apply standard unified diff without shell tools or host file access."""
    if not isinstance(diff, str):
        raise ArtifactError("unifiedDiff: expected text")
    if not diff:
        return before
    patch = lf_lines(diff.encode("utf-8", "strict"))
    if len(patch) < 3 or not patch[0].startswith(b"--- ") or not patch[1].startswith(b"+++ "):
        raise ArtifactError("diff: missing file headers")
    original = lf_lines(before)
    output: list[bytes] = []
    cursor = 0
    index = 2
    while index < len(patch):
        match = HUNK.fullmatch(patch[index])
        if match is None:
            raise ArtifactError("diff: expected hunk header")
        old_start, old_count, new_start, new_count = (
            int(match[1]), int(match[2] or b"1"), int(match[3]), int(match[4] or b"1")
        )
        old_index = old_start - 1 if old_count else old_start
        new_index = new_start - 1 if new_count else new_start
        if not cursor <= old_index <= len(original):
            raise ArtifactError("diff: invalid or overlapping old hunk position")
        output.extend(original[cursor:old_index])
        if len(output) != new_index:
            raise ArtifactError("diff: new hunk position does not match output")
        cursor = old_index
        operations: list[tuple[bytes, bytes]] = []
        index += 1
        while index < len(patch) and not patch[index].startswith(b"@@ "):
            line = patch[index]
            if line.rstrip(b"\n") == b"\\ No newline at end of file":
                if not operations or not operations[-1][1].endswith(b"\n"):
                    raise ArtifactError("diff: misplaced missing-newline marker")
                prefix, payload = operations[-1]
                operations[-1] = prefix, payload[:-1]
            elif line[:1] in (b" ", b"-", b"+"):
                if not line.endswith(b"\n"):
                    raise ArtifactError("diff: unterminated hunk line without marker")
                operations.append((line[:1], line[1:]))
            else:
                raise ArtifactError("diff: invalid hunk body line")
            index += 1
        consumed = produced = 0
        for prefix, payload in operations:
            if prefix in (b" ", b"-"):
                if cursor >= len(original) or original[cursor] != payload:
                    raise ArtifactError("diff: context/deletion does not match exact preimage")
                cursor += 1
                consumed += 1
            if prefix in (b" ", b"+"):
                output.append(payload)
                produced += 1
        if (consumed, produced) != (old_count, new_count):
            raise ArtifactError("diff: hunk counts do not match body")
    output.extend(original[cursor:])
    return b"".join(output)


def check_endpoints(preview: dict[str, Any], expected_before: bytes | None, expected_after: bytes) -> list[str]:
    errors: list[str] = []
    try:
        before_value = preview["beforeBytes"]
        before = None if before_value is None else bytes_field(before_value, "beforeBytes")
        after = bytes_field(preview["afterBytes"], "afterBytes")
        if before != expected_before:
            errors.append("before bytes differ from authored operation-point preimage")
        if after != expected_after:
            errors.append("after bytes differ from authored semantic oracle")
        if preview.get("basis") != "suppliedSnapshotConditional":
            errors.append("missing conditional supplied-snapshot basis")
        if preview.get("scope") != "operationPoint":
            errors.append("missing operation-point scope")
        check_replacements(before or b"", after, preview.get("replacements"))
        diff = preview.get("unifiedDiff")
        if apply_unified_diff(before or b"", diff) != after:
            errors.append("unified diff does not reconstruct exact after bytes")
        if (before or b"") == after and diff:
            errors.append("no-op has nonempty content diff")
    except (ArtifactError, KeyError, UnicodeError) as error:
        errors.append(str(error))
    return errors
