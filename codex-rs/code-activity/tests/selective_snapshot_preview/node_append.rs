//! Supplied byte snapshots are data, not filesystem reads or executed source.

use super::support::Fixture;
use codex_code_activity::SnapshotEpoch;
use codex_code_activity::SnapshotPreviewError;
use codex_code_activity::SnapshotPreviewLimit;
use codex_code_activity::TextEncoding;
use pretty_assertions::assert_eq;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const TWO: &str = "const fs=require('fs'); const fd=fs.openSync('out','a'); const alias=fd; fs.writeSync(alias,'β\\n'); fs.writeSync(fd,'尾',null,'utf8'); fs.closeSync(alias);";

#[test]
fn append_materializes_each_operation_preimage_and_exact_diff() -> TestResult {
    let first = Fixture::node(TWO, Some(b"start\n"))?.preview()?;
    assert_eq!(first.after_bytes, "start\nβ\n".as_bytes());
    assert_eq!(
        first.unified_diff,
        "--- supplied-before\n+++ conditional-after\n@@ -1,1 +1,2 @@\n start\n+β\n"
    );
    let next = Fixture::node_at(TWO, 1, Some(&first.after_bytes))?.preview()?;
    assert_eq!(next.after_bytes, "start\nβ\n尾".as_bytes());
    assert_eq!(next.filesystem_epoch, SnapshotEpoch::new(1));
    assert!(next.predecessor_conditions.contains(&first.evidence));
    for preview in [&first, &next] {
        let before = preview.before_bytes.as_ref().ok_or("present before")?;
        let replacement = preview.replacements[0];
        let reconstructed = [
            &before[..replacement.before.start],
            &preview.after_bytes[replacement.after.start..replacement.after.end],
            &before[replacement.before.end..],
        ]
        .concat();
        assert_eq!(reconstructed, preview.after_bytes);
    }
    Ok(())
}

#[test]
fn empty_append_keeps_bytes_but_still_requires_a_present_preimage() -> TestResult {
    let source = "const fs=require('fs'); const fd=fs.openSync('out','a'); fs.writeSync(fd,'');";
    let preview = Fixture::node(source, Some("é\r\n".as_bytes()))?.preview()?;
    assert_eq!(preview.after_bytes, "é\r\n".as_bytes());
    assert!(preview.replacements.is_empty());
    assert!(preview.unified_diff.is_empty());
    assert!(matches!(
        Fixture::node(source, None)?.preview(),
        Err(SnapshotPreviewError::MissingDescriptorPreimage)
    ));
    Ok(())
}

#[test]
fn append_refuses_unknown_preimage_invalid_utf8_and_wrong_epoch() -> TestResult {
    assert!(matches!(
        Fixture::node(TWO, None)?.preview(),
        Err(SnapshotPreviewError::MissingDescriptorPreimage)
    ));
    assert!(matches!(
        Fixture::node(TWO, Some(&[0xff]))?.preview(),
        Err(SnapshotPreviewError::InvalidUtf8)
    ));
    let mut later = Fixture::node_at(TWO, 1, Some("start\nβ\n".as_bytes()))?;
    later.input.before.binding.filesystem_epoch = SnapshotEpoch::new(0);
    assert!(matches!(
        later.preview(),
        Err(SnapshotPreviewError::TargetBindingMismatch)
    ));
    Ok(())
}

#[test]
fn append_limits_total_output_not_just_payload_bytes() -> TestResult {
    let mut fixture = Fixture::node(TWO, Some(b"start\n"))?;
    fixture.input.limits.output_bytes = 8;
    assert!(matches!(
        fixture.preview(),
        Err(SnapshotPreviewError::ResourceLimit {
            limit: SnapshotPreviewLimit::Output,
        })
    ));
    Ok(())
}

#[test]
fn captured_old_read_is_not_replaced_by_the_current_append_preimage() -> TestResult {
    let source = "const fs=require('fs'); const old=fs.readFileSync('out','utf8'); const fd=fs.openSync('out','a'); fs.writeSync(fd,'new'); fs.writeSync(fd,old);";
    let mut fixture = Fixture::node_at(source, 1, Some(b"oldnew"))?;
    fixture.read(0, 0, "out", b"old", TextEncoding::Utf8);
    assert_eq!(fixture.preview()?.after_bytes, b"oldnewold");
    fixture.input.reads[0].binding.filesystem_epoch = SnapshotEpoch::new(1);
    assert!(matches!(
        fixture.preview(),
        Err(SnapshotPreviewError::ReadBindingMismatch { .. })
    ));
    Ok(())
}

#[test]
fn fresh_read_after_append_requires_a_distinct_epoch_binding() -> TestResult {
    let source = "const fs=require('fs'); const fd=fs.openSync('out','a'); fs.writeSync(fd,'new'); const fresh=fs.readFileSync('out','utf8'); fs.writeSync(fd,fresh);";
    let mut fixture = Fixture::node_at(source, 1, Some(b"oldnew"))?;
    fixture.read(0, 1, "out", b"oldnew", TextEncoding::Utf8);
    assert_eq!(fixture.preview()?.after_bytes, b"oldnewoldnew");
    fixture.input.reads[0].binding.filesystem_epoch = SnapshotEpoch::new(0);
    assert!(matches!(
        fixture.preview(),
        Err(SnapshotPreviewError::ReadBindingMismatch { .. })
    ));
    Ok(())
}
