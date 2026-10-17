//! Exact descriptor bytes are authored; no host file or Node process is opened.

use super::support::Fixture;
use codex_code_activity::JavaScriptAssumption;
use codex_code_activity::SnapshotEpoch;
use codex_code_activity::SnapshotPreviewBasis;
use codex_code_activity::SnapshotPreviewDetails;
use codex_code_activity::SnapshotPreviewError;
use codex_code_activity::SnapshotPreviewLimit;
use codex_code_activity::SnapshotPreviewScope;
use codex_code_activity::TextEncoding;
use pretty_assertions::assert_eq;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn write(content: &str, position: &str, before: Option<&[u8]>) -> TestResult<Fixture> {
    let content = serde_json::to_string(content)?;
    Ok(Fixture::node(
        &format!(
            "const fs=require('fs'); const fd=fs.openSync('out','r+'); fs.writeSync(fd,{content},{position},'utf8'); fs.closeSync(fd);"
        ),
        before,
    )?)
}

#[test]
fn descriptor_overlay_preserves_suffix_and_exact_byte_offsets() -> TestResult {
    let preview = write("é", "0", Some("ab尾!\r\n".as_bytes()))?.preview()?;
    assert_eq!(preview.after_bytes, "é尾!\r\n".as_bytes());
    assert_eq!(
        preview.basis,
        SnapshotPreviewBasis::SuppliedSnapshotConditional
    );
    assert_eq!(preview.scope, SnapshotPreviewScope::OperationPoint);
    let SnapshotPreviewDetails::JavaScript { assumptions } = preview.details else {
        panic!("Node details")
    };
    assert!(assumptions.contains(&JavaScriptAssumption::CompleteDescriptorWrites));
    Ok(())
}

#[test]
fn descriptor_extension_zero_fills_gap_without_truncating_prior_bytes() -> TestResult {
    let preview = write("é", "4", Some(b"a"))?.preview()?;
    assert_eq!(preview.after_bytes, b"a\0\0\0\xc3\xa9");
    Ok(())
}

#[test]
fn zero_byte_write_at_huge_offset_preserves_file_length() -> TestResult {
    let mut fixture = write("", "9007199254740991", Some(b"same"))?;
    fixture.input.limits.output_bytes = 4;
    let preview = fixture.preview()?;
    assert_eq!(preview.after_bytes, b"same");
    assert!(preview.replacements.is_empty());
    Ok(())
}

#[test]
fn zero_byte_write_keeps_current_cursor_for_the_next_write() -> TestResult {
    let fixture = Fixture::node_at(
        "const fs=require('fs'); const fd=fs.openSync('out','r+'); fs.writeSync(fd,''); fs.writeSync(fd,'X');",
        1,
        Some(b"abc"),
    )?;
    assert_eq!(fixture.preview()?.after_bytes, b"Xbc");
    Ok(())
}

#[test]
fn open_truncation_requires_its_own_preimage_before_the_write_epoch() -> TestResult {
    let source = "const fs=require('fs'); const fd=fs.openSync('out','w'); fs.writeSync(fd,'fresh',0,'utf8'); fs.closeSync(fd);";
    let truncate = Fixture::node(source, Some(b"old content"))?.preview()?;
    assert!(truncate.after_bytes.is_empty());
    assert_eq!(truncate.filesystem_epoch, SnapshotEpoch::new(0));
    let mut write = Fixture::node_at(source, 1, Some(b""))?;
    let preview = write.preview()?;
    assert_eq!(preview.after_bytes, b"fresh");
    assert_eq!(preview.filesystem_epoch, SnapshotEpoch::new(1));
    assert!(preview.predecessor_conditions.contains(&truncate.evidence));
    write.input.before.binding.filesystem_epoch = SnapshotEpoch::new(0);
    assert!(matches!(
        write.preview(),
        Err(SnapshotPreviewError::TargetBindingMismatch)
    ));
    Ok(())
}

#[test]
fn cursor_and_explicit_position_transitions_materialize_separate_snapshots() -> TestResult {
    let source = "const fs=require('fs'); const fd=fs.openSync('out','r+'); fs.writeSync(fd,'é'); fs.writeSync(fd,'Z',5); fs.writeSync(fd,'😀',null); fs.writeSync(fd,'!',null);";
    for (ordinal, before, after) in [
        (0, "abcdef", "écdef"),
        (1, "écdef", "écdeZ"),
        (2, "écdeZ", "é😀"),
        (3, "é😀", "é😀!"),
    ] {
        let fixture = Fixture::node_at(source, ordinal, Some(before.as_bytes()))?;
        assert_eq!(
            fixture.preview()?.after_bytes,
            after.as_bytes(),
            "operation {ordinal}"
        );
    }
    Ok(())
}

#[test]
fn read_recipe_keeps_pre_open_snapshot_identity() -> TestResult {
    let mut fixture = Fixture::node_at(
        "const fs=require('fs'); const text=fs.readFileSync('out','utf8'); const fd=fs.openSync('out','w'); fs.writeSync(fd,text.replace('old','new'),0,'utf8');",
        1,
        Some(b""),
    )?;
    fixture.read(0, 0, "out", b"old\r\ntail", TextEncoding::Utf8);
    let preview = fixture.preview()?;
    assert_eq!(preview.after_bytes, b"new\r\ntail");
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
fn writes_splitting_utf8_boundaries_fail_without_lossy_text() -> TestResult {
    for offset in ["0", "1"] {
        assert!(matches!(
            write("X", offset, Some("éA".as_bytes()))?.preview(),
            Err(SnapshotPreviewError::InvalidUtf8)
        ));
    }
    Ok(())
}

#[test]
fn open_existing_descriptor_cannot_write_to_confirmed_absence() -> TestResult {
    assert!(matches!(
        write("X", "0", None)?.preview(),
        Err(SnapshotPreviewError::MissingDescriptorPreimage)
    ));
    Ok(())
}

#[test]
fn positioned_extension_is_bounded_before_allocating_gap() -> TestResult {
    let mut fixture = write("X", "1000000", Some(b"a"))?;
    fixture.input.limits.output_bytes = 64;
    assert!(matches!(
        fixture.preview(),
        Err(SnapshotPreviewError::ResourceLimit {
            limit: SnapshotPreviewLimit::Output,
        })
    ));
    Ok(())
}

#[test]
fn write_plus_materializes_open_truncation_and_separate_write_epoch() -> TestResult {
    let source = "const fs=require('fs'); const fd=fs.openSync('out','w+'); fs.writeSync(fd,'report: ' + 'ready\\n',null,'utf8'); fs.closeSync(fd);";
    for before in [Some(b"stale\n".as_slice()), None] {
        let truncate = Fixture::node(source, before)?.preview()?;
        assert!(truncate.after_bytes.is_empty());
        assert_eq!(truncate.filesystem_epoch, SnapshotEpoch::new(0));
        let mut write = Fixture::node_at(source, 1, Some(b""))?;
        let preview = write.preview()?;
        assert_eq!(preview.after_bytes, b"report: ready\n");
        assert_eq!(preview.scope, SnapshotPreviewScope::OperationPoint);
        assert_eq!(preview.filesystem_epoch, SnapshotEpoch::new(1));
        assert!(preview.predecessor_conditions.contains(&truncate.evidence));
        write.input.before.binding.filesystem_epoch = SnapshotEpoch::new(0);
        assert!(matches!(
            write.preview(),
            Err(SnapshotPreviewError::TargetBindingMismatch)
        ));
    }
    Ok(())
}

#[test]
fn write_plus_keeps_captured_read_from_before_its_truncation() -> TestResult {
    let mut fixture = Fixture::node_at(
        "const fs=require('fs'); const saved=fs.readFileSync('out','utf8'); const fd=fs.openSync('out','w+'); fs.writeSync(fd,saved,0,'utf8');",
        1,
        Some(b""),
    )?;
    fixture.read(0, 0, "out", b"captured\r\n", TextEncoding::Utf8);
    let preview = fixture.preview()?;
    assert_eq!(preview.after_bytes, b"captured\r\n");
    assert_eq!(
        preview.read_bindings[0].filesystem_epoch,
        SnapshotEpoch::new(0)
    );
    Ok(())
}
