"""Generated wire contracts; these tests never execute the represented Python source."""

import copy
import importlib.util
import json
import sys
from pathlib import Path
from types import ModuleType

import pytest
from pydantic import BaseModel, ValidationError

ROOT = Path(__file__).resolve().parents[1]
SCHEMA = ROOT.parents[1] / "codex-rs/app-server-protocol/schema/json"


@pytest.fixture
def models() -> ModuleType:
    path = ROOT / "scripts/update_sdk_artifacts.py"
    spec = importlib.util.spec_from_file_location("selective_python_logical_artifacts", path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module._load_generated_v2_all_module()


@pytest.fixture
def span() -> dict:
    return {
        "sourceId": 7,
        "startByte": 10,
        "endByte": 20,
        "startLine": 2,
        "endLine": 2,
        "startColumn": 0,
    }


@pytest.fixture
def read(span: dict) -> dict:
    return {
        "type": "read",
        "readId": 13,
        "filesystemEpoch": 0,
        "target": {"path": "result.txt", "cwd": None},
        "evidence": span,
        "encoding": "utf8",
    }


@pytest.fixture
def logical_write(span: dict) -> dict:
    return {
        "type": "logicalWrite",
        "streamId": 3,
        "openedAt": span,
        "mode": "updateExisting",
        "position": {
            "type": "rewoundZero",
            "readId": 13,
            "readEvidence": span,
            "seekEvidence": {**span, "startByte": 21, "endByte": 28},
        },
        "baseFilesystemEpoch": 0,
        "streamRevision": 0,
        "content": {"type": "literal", "value": "é\n", "evidence": span},
        "encoding": "utf8",
        "newline": "crLf",
        "requiredResult": "fullCharacterCount",
    }


@pytest.fixture
def snapshot(span: dict, logical_write: dict) -> dict:
    return {
        "basis": "preExecutionSnapshotConditional",
        "scope": "logicalStream",
        "sourceId": 7,
        "filesystemEpoch": 0,
        "before": "1234tail",
        "after": "é\r\ntail",
        "unifiedDiff": "",
        "readBindings": [],
        "textProfile": {
            "readNewline": "sourceDeclared",
            "writeNewline": "crLf",
            "runtimeDefaultEncoding": None,
        },
        "environmentId": "authored-sdk-wire-contract",
        "resolvedCwd": "/workspace",
        "resolvedTarget": "/workspace/result.txt",
        "replayOrdinals": [],
        "logicalStream": {
            "binding": {
                "sourceId": 7,
                "streamId": 3,
                "openedAt": span,
                "baseFilesystemEpoch": 0,
                "revision": 0,
            },
            "mode": "updateExisting",
            "position": logical_write["position"],
            "revisionAfter": 1,
            "payloadBytes": [195, 169, 13, 10],
            "acceptedCharactersRequired": 2,
            "realization": "requiresSuccessfulFlushOrClose",
        },
    }


def wire(model: BaseModel) -> dict:
    return model.model_dump(by_alias=True, mode="json", exclude_unset=True)


def test_legacy_omitted_newline_and_logical_metadata_remain_omitted(
    models: ModuleType, read: dict, snapshot: dict
) -> None:
    recipe = models.SelectiveTextRecipe.model_validate(read)
    operation_payload = {"type": "writeText", "content": read, "encoding": "utf8"}
    operation = models.SelectivePythonOperation.model_validate(operation_payload)
    assert recipe.root.newline is None
    assert "newline" not in recipe.root.model_fields_set
    assert not type(recipe.root).model_fields["newline"].is_required()
    assert operation.root.newline is None
    assert "newline" not in operation.root.model_fields_set
    assert wire(recipe) == read
    assert wire(operation) == operation_payload

    physical = {**snapshot, "scope": "operationPoint"}
    del physical["logicalStream"]
    model = models.SelectiveSnapshotPreview.model_validate(physical)
    assert model.logical_stream is None
    assert "logical_stream" not in model.model_fields_set
    assert wire(model) == physical


@pytest.mark.parametrize("newline", ["universalTranslate", "preserve"])
def test_explicit_read_policy_round_trip(models: ModuleType, read: dict, newline: str) -> None:
    payload = {**read, "newline": newline}
    model = models.SelectiveTextRecipe.model_validate(payload)
    assert model.root.newline.value == newline
    assert wire(model) == payload


@pytest.mark.parametrize("newline", ["runtimeDefault", "preserve", "cr", "crLf"])
def test_explicit_write_policy_round_trip(models: ModuleType, read: dict, newline: str) -> None:
    payload = {"type": "writeText", "content": read, "encoding": "utf8", "newline": newline}
    model = models.SelectivePythonOperation.model_validate(payload)
    assert model.root.newline.value == newline
    assert wire(model) == payload


def test_source_declared_and_cr_profile_round_trip(models: ModuleType) -> None:
    payload = {
        "readNewline": "sourceDeclared",
        "writeNewline": "cr",
        "runtimeDefaultEncoding": None,
    }
    model = models.SelectiveSnapshotTextProfile.model_validate(payload)
    assert model.read_newline.value == "sourceDeclared"
    assert model.write_newline.value == "cr"
    assert wire(model) == payload


@pytest.mark.parametrize(
    ("mode", "position"),
    [
        ("truncateWrite", "initialZero"),
        ("updateExisting", "initialZero"),
        ("updateExisting", "rewoundZero"),
    ],
)
def test_logical_union_is_typed_and_round_trips(
    models: ModuleType, logical_write: dict, mode: str, position: str
) -> None:
    payload = copy.deepcopy(logical_write)
    payload["mode"] = mode
    payload["baseFilesystemEpoch"] = 1 if mode == "truncateWrite" else 0
    if position == "initialZero":
        payload["position"] = {"type": "initialZero"}
    operation = models.SelectivePythonOperation.model_validate(payload)
    assert isinstance(operation.root, BaseModel)
    assert operation.root.type == "logicalWrite"
    assert isinstance(operation.root.position, models.SelectivePythonStreamPosition)
    assert isinstance(operation.root.content, models.SelectiveTextRecipe)
    assert isinstance(operation.root.required_result, models.SelectivePythonWriteResultCondition)
    assert operation.root.required_result.root == "fullCharacterCount"
    assert operation.root.stream_revision == 0
    assert wire(operation) == payload
    encoded = operation.model_dump_json(by_alias=True, exclude_unset=True)
    assert wire(models.SelectivePythonOperation.model_validate_json(encoded)) == payload


def test_logical_metadata_preserves_character_count_and_future_realization(
    models: ModuleType, snapshot: dict
) -> None:
    model = models.SelectiveSnapshotPreview.model_validate(snapshot)
    metadata = model.logical_stream
    assert isinstance(metadata, models.SelectiveLogicalStreamPreviewMetadata)
    assert isinstance(metadata.binding, models.SelectiveLogicalStreamBinding)
    assert isinstance(metadata.position, models.SelectivePythonStreamPosition)
    assert metadata.accepted_characters_required == 2
    assert len(metadata.payload_bytes) == 4
    assert all(isinstance(byte, models.PayloadByte) for byte in metadata.payload_bytes)
    assert [byte.root for byte in metadata.payload_bytes] == list("é\r\n".encode("utf-8"))
    assert wire(metadata)["payloadBytes"] == list("é\r\n".encode("utf-8"))
    assert isinstance(metadata.realization, models.SelectiveLogicalStreamRealization)
    assert metadata.realization.root == "requiresSuccessfulFlushOrClose"
    assert metadata.binding.revision == 0 and metadata.revision_after == 1
    assert wire(model) == snapshot
    encoded = model.model_dump_json(by_alias=True, exclude_unset=True)
    assert wire(models.SelectiveSnapshotPreview.model_validate_json(encoded)) == snapshot


def test_namespaces_and_captured_read_epoch_survive_enclosing_edit_round_trip(
    models: ModuleType, span: dict, read: dict, logical_write: dict, snapshot: dict
) -> None:
    # Source 7 / stream 3 / outer ordinal 19 / local ordinal 11 / seed epoch 1 /
    # captured read epoch 0 / stream revision 0->1 must not collapse into one counter.
    operation = {**logical_write, "baseFilesystemEpoch": 1, "mode": "truncateWrite"}
    operation["content"] = {**read, "newline": "preserve"}
    operation["position"] = {"type": "initialZero"}
    snapshot = copy.deepcopy(snapshot)
    snapshot.update(filesystemEpoch=1, before="", after="é\r\n", replayOrdinals=[17])
    snapshot["logicalStream"]["mode"] = "truncateWrite"
    snapshot["logicalStream"]["position"] = operation["position"]
    snapshot["logicalStream"]["binding"]["baseFilesystemEpoch"] = 1
    snapshot["readBindings"] = [
        {
            "sourceId": 7,
            "readId": 13,
            "filesystemEpoch": 0,
            "target": read["target"],
            "encoding": "utf8",
            "newline": "preserve",
        }
    ]
    payload = {
        "basis": "standardApiForecast",
        "carrierAssumptions": [],
        "details": {
            "kind": "python",
            "assumptions": [],
            "prediction": {
                "ordinal": 11,
                "evidence": span,
                "target": read["target"],
                "operation": operation,
                "dependencies": [],
                "predecessorConditions": [],
                "finalState": "notEstablished",
            },
        },
        "evidence": span,
        "laterContext": "unobserved",
        "ordinal": 19,
        "predecessorConditions": [],
        "snapshot": snapshot,
    }
    model = models.SelectivePredictedEdit.model_validate(payload)
    assert wire(model) == payload
    encoded = model.model_dump_json(by_alias=True, exclude_unset=True)
    assert wire(models.SelectivePredictedEdit.model_validate_json(encoded)) == payload


@pytest.mark.parametrize(
    "field",
    [
        "streamId",
        "openedAt",
        "mode",
        "position",
        "baseFilesystemEpoch",
        "streamRevision",
        "content",
        "encoding",
        "newline",
        "requiredResult",
    ],
)
def test_logical_write_requires_complete_contract(
    models: ModuleType, logical_write: dict, field: str
) -> None:
    del logical_write[field]
    with pytest.raises(ValidationError):
        models.SelectivePythonOperation.model_validate(logical_write)


@pytest.mark.parametrize(
    ("field", "value"),
    [
        ("requiredResult", "successfulWrite"),
        ("newline", "lf"),
        ("streamRevision", -1),
        ("baseFilesystemEpoch", -1),
        ("descriptorId", 3),
    ],
)
def test_logical_write_rejects_unknown_contracts_and_descriptor_fields(
    models: ModuleType, logical_write: dict, field: str, value: object
) -> None:
    logical_write[field] = value
    with pytest.raises(ValidationError):
        models.SelectivePythonOperation.model_validate(logical_write)


@pytest.mark.parametrize("field", ["readId", "readEvidence", "seekEvidence"])
def test_rewind_requires_read_and_seek_evidence(
    models: ModuleType, logical_write: dict, field: str
) -> None:
    del logical_write["position"][field]
    with pytest.raises(ValidationError):
        models.SelectivePythonOperation.model_validate(logical_write)


@pytest.mark.parametrize(
    "field", ["binding", "payloadBytes", "acceptedCharactersRequired", "realization"]
)
def test_logical_metadata_requires_conditions(
    models: ModuleType, snapshot: dict, field: str
) -> None:
    del snapshot["logicalStream"][field]
    with pytest.raises(ValidationError):
        models.SelectiveSnapshotPreview.model_validate(snapshot)


@pytest.mark.parametrize(
    ("field", "value"),
    [
        ("realization", "alreadyFlushed"),
        ("acceptedCharactersRequired", -1),
        ("payloadBytes", [256]),
    ],
)
def test_logical_metadata_rejects_invalid_condition_or_byte(
    models: ModuleType, snapshot: dict, field: str, value: object
) -> None:
    snapshot["logicalStream"][field] = value
    with pytest.raises(ValidationError):
        models.SelectiveSnapshotPreview.model_validate(snapshot)


def _references(value: object):
    if isinstance(value, dict):
        if "$ref" in value:
            yield value["$ref"]
        for item in value.values():
            yield from _references(item)
    elif isinstance(value, list):
        for item in value:
            yield from _references(item)


@pytest.mark.parametrize(
    "name", ["codex_app_server_protocol.v2.schemas.json", "ServerNotification.json"]
)
def test_schema_references_are_local_resolvable_and_retain_logical_types(name: str) -> None:
    bundle = json.loads((SCHEMA / name).read_text())
    references = set(_references(bundle))
    for reference in references:
        assert reference.startswith("#/"), reference
        target = bundle
        for part in reference[2:].split("/"):
            target = target[part.replace("~1", "/").replace("~0", "~")]
        assert isinstance(target, dict), reference
    for typename in [
        "SelectiveLogicalStreamPreviewMetadata",
        "SelectiveLogicalStreamBinding",
        "SelectivePythonStreamPosition",
        "SelectivePythonWriteResultCondition",
        "SelectiveLogicalStreamRealization",
        "SelectivePythonReadNewline",
        "SelectivePythonWriteNewline",
    ]:
        assert f"#/definitions/{typename}" in references
    definitions = bundle["definitions"]
    assert "sourceDeclared" in definitions["SelectiveSnapshotReadNewline"]["enum"]
    assert "cr" in definitions["SelectiveSnapshotWriteNewline"]["enum"]
    assert definitions["SelectivePythonWriteResultCondition"]["enum"] == ["fullCharacterCount"]
    assert definitions["SelectiveLogicalStreamRealization"]["enum"] == [
        "requiresSuccessfulFlushOrClose"
    ]
    preview = definitions["SelectiveSnapshotPreview"]
    assert "logicalStream" not in preview["required"]
    assert "#/definitions/SelectiveLogicalStreamPreviewMetadata" in set(
        _references(preview["properties"]["logicalStream"])
    )
