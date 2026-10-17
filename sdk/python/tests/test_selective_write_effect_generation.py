import importlib.util
import json
import sys
from pathlib import Path
from types import ModuleType

import pytest
from pydantic import ValidationError

ROOT = Path(__file__).resolve().parents[1]


@pytest.fixture
def artifacts() -> ModuleType:
    script_path = ROOT / "scripts" / "update_sdk_artifacts.py"
    spec = importlib.util.spec_from_file_location("selective_write_effect_artifacts", script_path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


@pytest.fixture
def models(artifacts: ModuleType) -> ModuleType:
    # Exercise the real generated module without importing the SDK runtime package.
    return artifacts._load_generated_v2_all_module()


@pytest.fixture
def edit_payload() -> dict:
    span = {
        "sourceId": 0,
        "startByte": 0,
        "endByte": 1,
        "startLine": 1,
        "endLine": 1,
        "startColumn": 0,
    }
    return {
        "ordinal": 0,
        "evidence": span,
        "target": {"path": "result.txt", "cwd": None},
        "content": {"type": "literal", "value": "new text", "evidence": span},
        "dependencies": [],
        "predecessorConditions": [],
    }


def test_omitted_write_effect_has_an_independent_typed_overwrite_default(
    models: ModuleType, edit_payload: dict
) -> None:
    first = models.SelectiveJavaScriptEdit.model_validate(edit_payload)
    second = models.SelectiveJavaScriptEdit.model_validate(edit_payload)

    assert isinstance(first.write_effect, models.SelectiveJavaScriptWriteEffect)
    assert isinstance(first.write_effect.root, models.OverwriteSelectiveJavaScriptWriteEffect)
    assert first.write_effect is not second.write_effect
    assert first.write_effect.root is not second.write_effect.root
    assert first.model_dump(by_alias=True, mode="json")["writeEffect"] == {"type": "overwrite"}
    assert not models.SelectiveJavaScriptEdit.model_fields["write_effect"].is_required()
    assert "write_effect" not in first.model_fields_set
    assert first.model_dump(by_alias=True, mode="json", exclude_unset=True) == edit_payload


@pytest.mark.parametrize(
    ("kind", "position"),
    [
        ("overwrite", None),
        ("truncate", None),
        ("descriptorAppend", None),
        ("descriptorWrite", "cursor"),
        ("descriptorWrite", "absolute"),
    ],
)
def test_explicit_write_effect_variants_round_trip(
    models: ModuleType, edit_payload: dict, kind: str, position: str | None
) -> None:
    effect = {"type": kind}
    if kind != "overwrite":
        effect.update(descriptorId=3, openedAt=edit_payload["evidence"])
    if kind == "descriptorWrite":
        effect.update(byteOffset=2, position=position)
    payload = {**edit_payload, "writeEffect": effect}

    edit = models.SelectiveJavaScriptEdit.model_validate(payload)

    assert "write_effect" in edit.model_fields_set
    assert edit.model_dump(by_alias=True, mode="json", exclude_unset=True) == payload


@pytest.mark.parametrize("field", ["writeEffect", "write_effect"])
def test_explicit_null_write_effect_is_rejected(
    models: ModuleType, edit_payload: dict, field: str
) -> None:
    with pytest.raises(ValidationError):
        models.SelectiveJavaScriptEdit.model_validate({**edit_payload, field: None})


def test_nullable_snapshot_and_target_fields_remain_nullable(
    models: ModuleType, edit_payload: dict
) -> None:
    payload = {
        "basis": "standardApiForecast",
        "carrierAssumptions": [],
        "details": {"kind": "javaScript", "assumptions": [], "prediction": edit_payload},
        "evidence": edit_payload["evidence"],
        "laterContext": "unobserved",
        "ordinal": 0,
        "predecessorConditions": [],
        "snapshot": None,
    }

    edit = models.SelectivePredictedEdit.model_validate(payload)

    assert edit.snapshot is None
    assert edit.model_dump(by_alias=True, mode="json", exclude_unset=True) == payload


def test_snapshot_absence_and_unresolved_codec_still_round_trip_as_null(models: ModuleType) -> None:
    payload = {
        "after": "new text",
        "basis": "preExecutionSnapshotConditional",
        "before": None,
        "environmentId": "authored-nullability-control",
        "filesystemEpoch": 0,
        "readBindings": [],
        "replayOrdinals": [],
        "resolvedCwd": "/workspace",
        "resolvedTarget": "/workspace/result.txt",
        "scope": "operationPoint",
        "sourceId": 0,
        "textProfile": {
            "readNewline": "preserve",
            "writeNewline": "preserve",
            "runtimeDefaultEncoding": None,
        },
        "unifiedDiff": "",
    }

    snapshot = models.SelectiveSnapshotPreview.model_validate(payload)

    assert snapshot.before is None
    assert snapshot.text_profile.runtime_default_encoding is None
    assert snapshot.model_dump(by_alias=True, mode="json", exclude_unset=True) == payload


def _write_schema(artifacts: ModuleType, schema_dir: Path, definitions: dict) -> None:
    artifacts.schema_bundle_path(schema_dir).write_text(json.dumps({"definitions": definitions}))


@pytest.mark.parametrize(
    "definitions",
    [{}, {"SelectiveJavaScriptEdit": {"properties": {"ordinal": {"type": "integer"}}}}],
)
def test_older_schemas_without_write_effect_leave_generated_source_untouched(
    artifacts: ModuleType, tmp_path: Path, definitions: dict
) -> None:
    _write_schema(artifacts, tmp_path, definitions)
    output = tmp_path / "generated.py"
    original = "# Generated source predating writeEffect.\n"
    output.write_text(original)

    artifacts._preserve_javascript_write_effect_default(output, tmp_path)

    assert output.read_text() == original


@pytest.mark.parametrize("change", ["nullable", "required", "different-reference"])
def test_changed_write_effect_schema_fails_without_rewriting_output(
    artifacts: ModuleType, tmp_path: Path, change: str
) -> None:
    reference = {"$ref": "#/definitions/SelectiveJavaScriptWriteEffect"}
    definition = {"properties": {"writeEffect": reference}, "required": []}
    if change == "nullable":
        definition["properties"]["writeEffect"] = {"anyOf": [reference, {"type": "null"}]}
    elif change == "required":
        definition["required"] = ["writeEffect"]
    else:
        definition["properties"]["writeEffect"] = {"$ref": "#/definitions/OtherEffect"}
    _write_schema(artifacts, tmp_path, {"SelectiveJavaScriptEdit": definition})
    output = tmp_path / "generated.py"
    original = "# Preserve this output on schema drift.\n"
    output.write_text(original)

    with pytest.raises(RuntimeError, match="optional non-null reference"):
        artifacts._preserve_javascript_write_effect_default(output, tmp_path)

    assert output.read_text() == original


@pytest.mark.parametrize(
    ("change", "message"),
    [
        ("missing-target", "unique SelectiveJavaScriptEdit"),
        ("duplicate-target", "unique SelectiveJavaScriptEdit"),
        ("missing-default-model", "unique OverwriteSelectiveJavaScriptWriteEffect"),
        ("unexpected-field", "unexpected shape"),
    ],
)
def test_changed_generated_model_shape_fails_without_rewriting_output(
    artifacts: ModuleType, tmp_path: Path, change: str, message: str
) -> None:
    _write_schema(
        artifacts,
        tmp_path,
        {
            "SelectiveJavaScriptEdit": {
                "properties": {
                    "writeEffect": {"$ref": "#/definitions/SelectiveJavaScriptWriteEffect"}
                }
            }
        },
    )
    # Deliberately invalid generator outputs are inert text, never imported.
    source = (
        "class SelectiveJavaScriptEdit(BaseModel):\n    write_effect: int | None = None\n\n"
        "class SelectiveJavaScriptWriteEffect(RootModel):\n    pass\n\n"
        "class OverwriteSelectiveJavaScriptWriteEffect(BaseModel):\n    pass\n"
    )
    if change == "missing-target":
        source = source.replace("class SelectiveJavaScriptEdit(", "class OtherEdit(")
    elif change == "duplicate-target":
        source += "\n\nclass SelectiveJavaScriptEdit(BaseModel):\n    pass\n"
    elif change == "missing-default-model":
        source = source.replace(
            "class OverwriteSelectiveJavaScriptWriteEffect(", "class OtherEffect("
        )
    output = tmp_path / "generated.py"
    output.write_text(source)

    with pytest.raises(RuntimeError, match=message):
        artifacts._preserve_javascript_write_effect_default(output, tmp_path)

    assert output.read_text() == source


@pytest.mark.parametrize("field", ["descriptorId", "openedAt", "type"])
def test_append_requires_identity_opening_and_tag(
    models: ModuleType, edit_payload: dict, field: str
) -> None:
    effect = {"type": "descriptorAppend", "descriptorId": 3, "openedAt": edit_payload["evidence"]}
    del effect[field]
    with pytest.raises(ValidationError):
        models.SelectiveJavaScriptEdit.model_validate({**edit_payload, "writeEffect": effect})


@pytest.mark.parametrize("field,value", [("byteOffset", 0), ("position", "cursor")])
def test_append_rejects_positioned_write_fields(
    models: ModuleType, edit_payload: dict, field: str, value: object
) -> None:
    effect = {"type": "descriptorAppend", "descriptorId": 3, "openedAt": edit_payload["evidence"]}
    effect[field] = value
    with pytest.raises(ValidationError):
        models.SelectiveJavaScriptEdit.model_validate({**edit_payload, "writeEffect": effect})
