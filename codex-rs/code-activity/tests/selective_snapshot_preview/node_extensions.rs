//! Exact expected bytes are authored directly; no JavaScript runtime is invoked.

use super::support::Fixture;
use codex_code_activity::SnapshotPreviewBasis;
use codex_code_activity::SnapshotPreviewError;
use codex_code_activity::SnapshotPreviewLimit;
use codex_code_activity::SnapshotPreviewScope;
use codex_code_activity::SnapshotReadNewline;
use codex_code_activity::TextEncoding;
use pretty_assertions::assert_eq;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn transform(old: &str, new: &str, bytes: &[u8]) -> TestResult<Fixture> {
    let old = serde_json::to_string(old)?;
    let new = serde_json::to_string(new)?;
    let source = format!(
        "const {{readFileSync: read, writeFileSync: write}} = require('node:fs'); const s = read('input', 'utf8'); write('output', s.replaceAll({old}, {new}), 'utf8');"
    );
    let mut fixture = Fixture::node(&source, Some(bytes))?;
    fixture.read(0, 0, "input", bytes, TextEncoding::Utf8);
    Ok(fixture)
}

#[test]
fn all_matches_are_nonoverlapping_and_use_literal_scalar_strings() -> TestResult {
    for (old, new, input, expected) in [
        ("aa", "b", "aaaaa", "bba"),
        ("aba", "X", "ababa", "Xba"),
        ("😀", "é", "😀😀x", "ééx"),
        ("é界", "😀", "é界é界", "😀😀"),
        ("a", "aa", "aa", "aaaa"),
        ("a", "", "banana", "bnn"),
        ("z", "x", "unchanged", "unchanged"),
        ("a", "x", "", ""),
    ] {
        let preview = transform(old, new, input.as_bytes())?
            .preview()
            .expect("bounded Node recipe");
        assert_eq!(
            preview.after_bytes,
            expected.as_bytes(),
            "{input:?}, {old:?}, {new:?}"
        );
        assert_eq!(
            preview.basis,
            SnapshotPreviewBasis::SuppliedSnapshotConditional
        );
        assert_eq!(preview.scope, SnapshotPreviewScope::OperationPoint);
        assert_eq!(preview.read_bindings.len(), 1);
        assert_eq!(preview.predecessor_conditions.len(), 2);
    }
    Ok(())
}

#[test]
fn node_all_replacements_preserve_crlf_and_reconstruct_exact_bytes() -> TestResult {
    let fixture = transform("old", "new", b"old\r\nold\rold\n")?;
    let preview = fixture.preview().expect("Node newline preservation");
    assert_eq!(preview.after_bytes, b"new\r\nnew\rnew\n");
    let range = preview.replacements[0];
    let before = preview.before_bytes.expect("supplied preimage");
    let reconstructed = [
        &before[..range.before.start],
        &preview.after_bytes[range.after.start..range.after.end],
        &before[range.before.end..],
    ]
    .concat();
    assert_eq!(reconstructed, preview.after_bytes);
    Ok(())
}

#[test]
fn all_replacement_output_growth_is_bounded_before_allocation() -> TestResult {
    let mut fixture = transform("x", "0123456789", b"xxxxxx")?;
    fixture.input.limits.output_bytes = 32;
    assert!(matches!(
        fixture.preview(),
        Err(SnapshotPreviewError::ResourceLimit {
            limit: SnapshotPreviewLimit::Output,
        })
    ));
    Ok(())
}

#[test]
fn destructured_reads_still_require_matched_snapshot_and_node_newline_premises() -> TestResult {
    let mut fixture = transform("a", "b", b"a\r\na")?;
    fixture.input.reads.clear();
    assert!(matches!(
        fixture.preview(),
        Err(SnapshotPreviewError::MissingRead { .. })
    ));
    fixture.read(0, 0, "input", b"a\r\na", TextEncoding::Utf8);
    fixture.input.reads[0].binding.newline = SnapshotReadNewline::PythonUniversal;
    assert!(matches!(
        fixture.preview(),
        Err(SnapshotPreviewError::ReadBindingMismatch { .. })
    ));
    Ok(())
}

#[test]
fn saved_filesystem_capabilities_materialize_authored_bytes_and_exact_diff() -> TestResult {
    let source = "let fs=require('node:fs'); let read=fs.readFileSync; let write=fs.writeFileSync; const savedRead=read; const savedWrite=write; fs=null; read='changed'; write='changed'; const text=savedRead('input','utf8'); savedWrite('output',text.replaceAll('old','new'));";
    let mut fixture = Fixture::node(source, Some(b"old\n"))?;
    fixture.read(0, 0, "input", b"old\n", TextEncoding::Utf8);
    let preview = fixture.preview()?;
    assert_eq!(preview.after_bytes, b"new\n");
    assert_eq!(
        preview.unified_diff,
        "--- supplied-before\n+++ conditional-after\n@@ -1,1 +1,1 @@\n-old\n+new\n"
    );
    assert_eq!(
        preview.basis,
        SnapshotPreviewBasis::SuppliedSnapshotConditional
    );
    assert_eq!(preview.scope, SnapshotPreviewScope::OperationPoint);
    assert_eq!(preview.read_bindings.len(), 1);
    assert_eq!(preview.predecessor_conditions.len(), 2);
    Ok(())
}

#[test]
fn method_aliases_cannot_bypass_snapshot_read_binding_checks() -> TestResult {
    let source = "const fs=require('fs'); const read=fs.readFileSync; const write=fs.writeFileSync; const text=read('input','utf8'); write('output',text);";
    let mut fixture = Fixture::node(source, Some(b"old\n"))?;
    assert!(matches!(
        fixture.preview(),
        Err(SnapshotPreviewError::MissingRead { .. })
    ));
    fixture.read(0, 1, "input", b"old\n", TextEncoding::Utf8);
    assert!(matches!(
        fixture.preview(),
        Err(SnapshotPreviewError::ReadBindingMismatch { .. })
    ));
    Ok(())
}
