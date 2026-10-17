//! UTF-16 expectations are authored; source programs and paths are never executed.

use super::support::Fixture;
use codex_code_activity::SnapshotEpoch;
use codex_code_activity::SnapshotPreviewBasis;
use codex_code_activity::SnapshotPreviewError;
use codex_code_activity::SnapshotPreviewLimit;
use codex_code_activity::SnapshotPreviewScope;
use codex_code_activity::TextEncoding;
use pretty_assertions::assert_eq;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn literal(text: &str, arguments: &str) -> TestResult<Fixture> {
    let text = serde_json::to_string(text)?;
    Ok(Fixture::node(
        &format!("require('fs').writeFileSync('out',{text}.slice({arguments}),'utf8');"),
        Some(b"before"),
    )?)
}

#[test]
fn utf16_pair_slice_preserves_exact_descriptor_overlay_bytes() -> TestResult {
    let source = "const fs=require('node:fs'); const marking=fs.readFileSync('marking.txt','utf8'); const fd=fs.openSync('marking.txt','r+'); fs.writeSync(fd,marking.slice(0,2)+'!',0,'utf8'); fs.closeSync(fd);";
    let mut fixture = Fixture::node(source, Some("🦉?tail\n".as_bytes()))?;
    fixture.read(
        0,
        0,
        "marking.txt",
        "🦉?tail\n".as_bytes(),
        TextEncoding::Utf8,
    );
    let preview = fixture.preview()?;
    assert_eq!(preview.after_bytes, "🦉!tail\n".as_bytes());
    assert_eq!(
        preview.basis,
        SnapshotPreviewBasis::SuppliedSnapshotConditional
    );
    assert_eq!(preview.scope, SnapshotPreviewScope::OperationPoint);
    assert_eq!(preview.read_bindings.len(), 1);
    assert_eq!(
        preview.read_bindings[0].filesystem_epoch,
        SnapshotEpoch::new(0)
    );
    Ok(())
}

#[test]
fn nonnegative_utf16_bounds_clamp_and_empty_intervals_stay_empty() -> TestResult {
    for (arguments, expected) in [
        ("0,3", "a🦉"),
        ("1,3", "🦉"),
        ("3", "b界"),
        ("3,4", "b"),
        ("0,99", "a🦉b界"),
        ("5,99", ""),
        ("99", ""),
        ("4,3", ""),
        ("2,2", ""),
        ("2,1", ""),
        ("0,0", ""),
    ] {
        assert_eq!(
            literal("a🦉b界", arguments)?.preview()?.after_bytes,
            expected.as_bytes(),
            "{arguments}"
        );
    }
    assert_eq!(
        literal("e\u{0301}x", "1,2")?.preview()?.after_bytes,
        "\u{0301}".as_bytes()
    );
    Ok(())
}

#[test]
fn nonempty_slices_splitting_surrogates_refuse_without_lossy_replacement() -> TestResult {
    for arguments in ["0,2", "2,3", "2", "1,2"] {
        assert!(
            matches!(
                literal("a🦉b", arguments)?.preview(),
                Err(SnapshotPreviewError::UnsupportedOperation)
            ),
            "{arguments}"
        );
    }
    Ok(())
}

#[test]
fn slice_composes_with_scalar_replacements_and_concatenation() -> TestResult {
    let fixture = Fixture::node(
        "const fs=require('fs'); fs.writeFileSync('out',('🦉old'+'old').replaceAll('old','new').slice(0,5));",
        Some(b"before"),
    )?;
    assert_eq!(fixture.preview()?.after_bytes, "🦉new".as_bytes());
    Ok(())
}

#[test]
fn captured_slice_uses_its_original_read_and_bound_versions() -> TestResult {
    let source = "const fs=require('fs'); let text=fs.readFileSync('input','utf8'); let start=0; let end=2; const saved=text.slice(start,end); start=1; end=99; text='changed'; fs.writeFileSync('input','changed'); fs.writeFileSync('out',saved);";
    let mut fixture = Fixture::node_at(source, 1, Some(b"before"))?;
    fixture.read(0, 0, "input", "🦉tail".as_bytes(), TextEncoding::Utf8);
    let preview = fixture.preview()?;
    assert_eq!(preview.after_bytes, "🦉".as_bytes());
    assert_eq!(
        preview.read_bindings[0].filesystem_epoch,
        SnapshotEpoch::new(0)
    );
    fixture.input.reads[0].binding.filesystem_epoch = SnapshotEpoch::new(1);
    assert!(matches!(
        fixture.preview(),
        Err(SnapshotPreviewError::ReadBindingMismatch { .. })
    ));
    Ok(())
}

#[test]
fn even_an_empty_slice_requires_its_captured_input_read() -> TestResult {
    let source = "const fs=require('fs'); const text=fs.readFileSync('input','utf8'); fs.writeFileSync('out',text.slice(99,0));";
    let mut fixture = Fixture::node(source, Some(b"before"))?;
    assert!(matches!(
        fixture.preview(),
        Err(SnapshotPreviewError::MissingRead { .. })
    ));
    fixture.read(0, 0, "input", b"data", TextEncoding::Utf8);
    assert!(fixture.preview()?.after_bytes.is_empty());
    Ok(())
}

#[test]
fn slice_preserves_depth_work_and_intermediate_output_limits() -> TestResult {
    for (limit, expected) in [
        ("depth", SnapshotPreviewLimit::Depth),
        ("work", SnapshotPreviewLimit::Work),
        ("output", SnapshotPreviewLimit::Output),
    ] {
        let mut fixture = literal("abcdef", "0,1")?;
        match limit {
            "depth" => fixture.input.limits.depth = 1,
            "work" => fixture.input.limits.work = 1,
            _ => fixture.input.limits.output_bytes = 1,
        }
        assert!(matches!(
            fixture.preview(),
            Err(SnapshotPreviewError::ResourceLimit { limit }) if limit == expected
        ));
    }
    Ok(())
}

#[test]
fn legacy_literal_surrogate_split_refuses_concrete_preview() -> TestResult {
    let fixture = Fixture::node(
        "require('fs').writeFileSync('out','😀a'.slice(1));",
        Some(b"before"),
    )?;
    assert_eq!(fixture.analysis.edits().len(), 1);
    assert!(matches!(
        fixture.preview(),
        Err(SnapshotPreviewError::UnsupportedOperation)
    ));
    Ok(())
}

#[test]
fn captured_surrogate_split_refuses_after_read_binding_without_erasing_prefix() -> TestResult {
    let source = "const fs=require('fs'); const text=fs.readFileSync('input','utf8'); fs.writeFileSync('out','ok'); fs.writeFileSync('out',text.slice(1));";
    assert_eq!(
        Fixture::node(source, Some(b"before"))?
            .preview()?
            .after_bytes,
        b"ok"
    );
    let mut fixture = Fixture::node_at(source, 1, Some(b"ok"))?;
    assert!(matches!(
        fixture.preview(),
        Err(SnapshotPreviewError::MissingRead { .. })
    ));
    fixture.read(0, 0, "input", "😀a".as_bytes(), TextEncoding::Utf8);
    assert!(matches!(
        fixture.preview(),
        Err(SnapshotPreviewError::UnsupportedOperation)
    ));
    Ok(())
}
