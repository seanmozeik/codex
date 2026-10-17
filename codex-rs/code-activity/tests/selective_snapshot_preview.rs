//! Authored source is parsed only; snapshot bytes are never read from paths.

#[path = "selective_snapshot_preview/node_append.rs"]
mod node_append;
#[path = "selective_snapshot_preview/node_descriptors.rs"]
mod node_descriptors;
#[path = "selective_snapshot_preview/node_extensions.rs"]
mod node_extensions;
#[path = "selective_snapshot_preview/node_slices.rs"]
mod node_slices;
#[path = "selective_snapshot_preview/recipes.rs"]
mod recipes;
#[path = "selective_snapshot_preview/support.rs"]
mod support;

use codex_code_activity::ActivityError;
use codex_code_activity::SnapshotEncoding;
use codex_code_activity::SnapshotEpoch;
use codex_code_activity::SnapshotPreviewBasis;
use codex_code_activity::SnapshotPreviewDetails;
use codex_code_activity::SnapshotPreviewError;
use codex_code_activity::SnapshotPreviewLimit;
use codex_code_activity::SnapshotPreviewScope;
use codex_code_activity::SnapshotTextProfile;
use codex_code_activity::SnapshotWriteNewline;
use codex_code_activity::TextEncoding;
use pretty_assertions::assert_eq;
use support::Fixture;

const REPLACE: &str = "from pathlib import Path\ns = Path('input').read_text(encoding='utf-8')\nPath('output').write_text(s.replace('old', 'new'), encoding='utf-8')";

fn replace_fixture() -> Result<Fixture, ActivityError> {
    let mut fixture = Fixture::python(REPLACE, 0, Some(b"old\n"))?;
    fixture.read(0, 0, "input", b"old\n", TextEncoding::Utf8);
    Ok(fixture)
}

#[test]
fn exact_artifact_retains_scope_prerequisites_and_reconstruction_ranges()
-> Result<(), ActivityError> {
    let fixture = replace_fixture()?;
    let preview = fixture.preview().expect("conditional preview");
    assert_eq!(
        preview.basis,
        SnapshotPreviewBasis::SuppliedSnapshotConditional
    );
    assert_eq!(preview.scope, SnapshotPreviewScope::OperationPoint);
    assert_eq!(&preview.after_bytes, b"new\n");
    let replacement = preview.replacements[0];
    let before = preview.before_bytes.as_ref().expect("supplied before");
    let reconstructed = [
        &before[..replacement.before.start],
        &preview.after_bytes[replacement.after.start..replacement.after.end],
        &before[replacement.before.end..],
    ]
    .concat();
    assert_eq!(reconstructed, preview.after_bytes);
    assert_eq!(
        preview.unified_diff,
        "--- supplied-before\n+++ conditional-after\n@@ -1,1 +1,1 @@\n-old\n+new\n"
    );
    assert!(!preview.predecessor_conditions.is_empty());
    Ok(())
}

#[test]
fn later_opaque_source_keeps_operation_point_preview_and_uncertainty() -> Result<(), ActivityError>
{
    let mut fixture = Fixture::python(&format!("{REPLACE}\nunknown()"), 0, Some(b"old\n"))?;
    fixture.read(0, 0, "input", b"old\n", TextEncoding::Utf8);
    let preview = fixture.preview().expect("operation point");
    assert_eq!(preview.after_bytes, b"new\n");
    assert!(matches!(
        preview.details,
        SnapshotPreviewDetails::Python {
            final_state: codex_code_activity::PythonFinalState::OpaqueSuccessor,
            ..
        }
    ));
    Ok(())
}

#[test]
fn missing_read_is_a_typed_refusal() -> Result<(), ActivityError> {
    let fixture = Fixture::python(REPLACE, 0, None)?;
    assert!(matches!(
        fixture.preview(),
        Err(SnapshotPreviewError::MissingRead { .. })
    ));
    Ok(())
}

#[test]
fn call_source_epoch_target_codec_and_newline_bindings_are_checked() -> Result<(), ActivityError> {
    let mut fixture = replace_fixture()?;
    fixture.input.call_id = "other-call".into();
    assert!(matches!(
        fixture.preview(),
        Err(SnapshotPreviewError::CallMismatch)
    ));
    fixture.input.call_id = fixture.analysis.call_id().clone();
    fixture.input.before.binding.filesystem_epoch = SnapshotEpoch::new(1);
    assert!(matches!(
        fixture.preview(),
        Err(SnapshotPreviewError::TargetBindingMismatch)
    ));
    fixture.input.before.binding.filesystem_epoch = SnapshotEpoch::new(0);
    fixture.input.reads[0].binding.filesystem_epoch = SnapshotEpoch::new(1);
    assert!(matches!(
        fixture.preview(),
        Err(SnapshotPreviewError::ReadBindingMismatch { .. })
    ));
    fixture.input.reads[0].binding.filesystem_epoch = SnapshotEpoch::new(0);
    fixture.input.reads[0].binding.encoding = TextEncoding::RuntimeDefault;
    assert!(matches!(
        fixture.preview(),
        Err(SnapshotPreviewError::ReadBindingMismatch { .. })
    ));
    fixture.input.reads[0].binding.encoding = TextEncoding::Utf8;
    fixture.input.reads[0].binding.newline = codex_code_activity::SnapshotReadNewline::Preserve;
    assert!(matches!(
        fixture.preview(),
        Err(SnapshotPreviewError::ReadBindingMismatch { .. })
    ));
    Ok(())
}

#[test]
fn exact_path_spelling_is_not_normalized_or_aliased() -> Result<(), ActivityError> {
    let mut fixture = replace_fixture()?;
    if let codex_code_activity::Target::Literal { path, .. } =
        &mut fixture.input.reads[0].binding.target
    {
        *path = "./input".into();
    }
    assert!(matches!(
        fixture.preview(),
        Err(SnapshotPreviewError::ReadBindingMismatch { .. })
    ));
    Ok(())
}

#[test]
fn duplicate_read_assertions_are_rejected() -> Result<(), ActivityError> {
    let mut fixture = replace_fixture()?;
    fixture.input.reads.push(fixture.input.reads[0].clone());
    assert!(matches!(
        fixture.preview(),
        Err(SnapshotPreviewError::DuplicateRead { .. })
    ));
    Ok(())
}

#[test]
fn contradictory_same_epoch_preimage_is_rejected() -> Result<(), ActivityError> {
    let source = REPLACE.replace("Path('output')", "Path('input')");
    let mut fixture = Fixture::python(&source, 0, Some(b"different"))?;
    fixture.read(0, 0, "input", b"old\n", TextEncoding::Utf8);
    assert!(matches!(
        fixture.preview(),
        Err(SnapshotPreviewError::ConflictingSnapshots { .. })
    ));
    Ok(())
}

#[test]
fn runtime_default_encoding_requires_explicit_resolution() -> Result<(), ActivityError> {
    let mut fixture = Fixture::python(
        "from pathlib import Path\nPath('output').write_text('hello')",
        0,
        None,
    )?;
    assert!(matches!(
        fixture.preview(),
        Err(SnapshotPreviewError::UnresolvedEncoding)
    ));
    fixture.input.text_profile = fixture
        .input
        .text_profile
        .with_runtime_default_encoding(SnapshotEncoding::Utf8);
    assert_eq!(
        fixture.preview().expect("explicit default").after_bytes,
        b"hello"
    );
    Ok(())
}

#[test]
fn default_profile_does_not_guess_host_newlines() -> Result<(), ActivityError> {
    let mut fixture = replace_fixture()?;
    fixture.input.text_profile = SnapshotTextProfile::default();
    assert!(matches!(
        fixture.preview(),
        Err(SnapshotPreviewError::NewlineProfileMismatch)
    ));
    Ok(())
}

#[test]
fn python_universal_read_and_crlf_write_operate_on_raw_bytes() -> Result<(), ActivityError> {
    let mut fixture = Fixture::python(
        "from pathlib import Path\ns = Path('input').read_text(encoding='utf-8')\nPath('output').write_text(s, encoding='utf-8')",
        0,
        None,
    )?;
    fixture.read(0, 0, "input", b"a\r\nb\rc\n", TextEncoding::Utf8);
    fixture.input.text_profile.write_newline = SnapshotWriteNewline::CrLf;
    assert_eq!(
        fixture.preview().expect("explicit CRLF host").after_bytes,
        b"a\r\nb\r\nc\r\n"
    );
    Ok(())
}

#[test]
fn invalid_utf8_never_turns_into_lossy_replacement_text() -> Result<(), ActivityError> {
    let mut fixture = replace_fixture()?;
    fixture.input.reads[0].bytes = vec![0xff];
    assert!(matches!(
        fixture.preview(),
        Err(SnapshotPreviewError::InvalidUtf8)
    ));
    Ok(())
}

#[test]
fn intermediate_expansion_is_refused_before_allocating_output() -> Result<(), ActivityError> {
    let source = "from pathlib import Path\ns = Path('input').read_text(encoding='utf-8')\nPath('output').write_text(s.replace('', '0123456789'), encoding='utf-8')";
    let mut fixture = Fixture::python(source, 0, None)?;
    fixture.read(0, 0, "input", b"abcdef", TextEncoding::Utf8);
    fixture.input.limits.output_bytes = 32;
    assert!(matches!(
        fixture.preview(),
        Err(SnapshotPreviewError::ResourceLimit {
            limit: SnapshotPreviewLimit::Output
        })
    ));
    Ok(())
}

#[test]
fn diff_work_and_depth_limits_have_explicit_refusals() -> Result<(), ActivityError> {
    let mut fixture = replace_fixture()?;
    fixture.input.limits.diff_bytes = 8;
    assert!(matches!(
        fixture.preview(),
        Err(SnapshotPreviewError::ResourceLimit {
            limit: SnapshotPreviewLimit::Diff
        })
    ));
    fixture.input.limits.diff_bytes = 4096;
    fixture.input.limits.depth = 1;
    assert!(matches!(
        fixture.preview(),
        Err(SnapshotPreviewError::ResourceLimit {
            limit: SnapshotPreviewLimit::Depth
        })
    ));
    fixture.input.limits.depth = 64;
    fixture.input.limits.work = 1;
    assert!(matches!(
        fixture.preview(),
        Err(SnapshotPreviewError::ResourceLimit {
            limit: SnapshotPreviewLimit::Work
        })
    ));
    Ok(())
}

#[test]
fn node_first_replace_and_newline_preservation_are_separate_from_python()
-> Result<(), ActivityError> {
    let mut fixture = Fixture::node(
        "const fs = require('node:fs'); const s = fs.readFileSync('input', 'utf8'); fs.writeFileSync('output', s.replace('old', 'new'), 'utf8');",
        None,
    )?;
    fixture.read(0, 0, "input", b"old\r\nold\n", TextEncoding::Utf8);
    let preview = fixture.preview().expect("conditional Node preview");
    assert_eq!(preview.after_bytes, b"new\r\nold\n");
    assert!(matches!(
        preview.details,
        SnapshotPreviewDetails::JavaScript { .. }
    ));
    assert!(!preview.predecessor_conditions.is_empty());
    Ok(())
}

#[test]
fn loop_occurrence_is_preserved_in_materialized_details() -> Result<(), ActivityError> {
    let fixture = Fixture::python(
        "from pathlib import Path\nfor p in ['first', 'second']:\n    Path(p).write_text('ok', encoding='utf-8')",
        1,
        None,
    )?;
    let preview = fixture.preview().expect("second occurrence");
    let SnapshotPreviewDetails::Python {
        iteration_context, ..
    } = preview.details
    else {
        panic!("Python details")
    };
    assert_eq!(iteration_context[0].index, 1);
    Ok(())
}
