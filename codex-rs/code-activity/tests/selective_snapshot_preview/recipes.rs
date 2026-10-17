use super::support::Fixture;
use codex_code_activity::ActivityError;
use codex_code_activity::SnapshotPreviewError;
use codex_code_activity::SnapshotPreviewLimit;
use codex_code_activity::TextEncoding;
use pretty_assertions::assert_eq;

fn transform(expression: &str, bytes: &[u8]) -> Result<Fixture, ActivityError> {
    let source = format!(
        "from pathlib import Path\ns = Path('input').read_text(encoding='utf-8')\nPath('output').write_text({expression}, encoding='utf-8')"
    );
    let mut fixture = Fixture::python(&source, 0, None)?;
    fixture.read(0, 0, "input", bytes, TextEncoding::Utf8);
    Ok(fixture)
}

#[test]
fn slices_use_unicode_codepoints_and_python_negative_clamping() -> Result<(), ActivityError> {
    for (expression, expected) in [
        ("s[1:-1]", "界"),
        ("s[-99:99]", "é界尾"),
        ("s[2:1]", ""),
        ("s[99:]", ""),
        ("s[:-99]", ""),
    ] {
        let fixture = transform(expression, "é界尾".as_bytes())?;
        assert_eq!(
            fixture.preview().expect(expression).after_bytes,
            expected.as_bytes(),
            "{expression}"
        );
    }
    Ok(())
}

#[test]
fn captured_index_slice_and_dynamic_replace_compute_codepoint_positions()
-> Result<(), ActivityError> {
    let source = "from pathlib import Path\ns = Path('input').read_text(encoding='utf-8')\ni = s.index('START')\nj = s.index('END', i)\nold = s[i:j]\nPath('output').write_text(s.replace(old, old + '!'), encoding='utf-8')";
    let mut fixture = Fixture::python(source, 0, None)?;
    fixture.read(
        0,
        0,
        "input",
        "éSTART界END尾".as_bytes(),
        TextEncoding::Utf8,
    );
    assert_eq!(
        fixture.preview().expect("dynamic recipe").after_bytes,
        "éSTART界!END尾".as_bytes()
    );
    Ok(())
}

#[test]
fn empty_replace_inserts_at_codepoint_boundaries_and_honors_count() -> Result<(), ActivityError> {
    for (expression, expected) in [
        ("s.replace('', 'x')", "xéx界x"),
        ("s.replace('', 'x', 2)", "xéx界"),
        ("s.replace('', 'x', 0)", "é界"),
    ] {
        assert_eq!(
            transform(expression, "é界".as_bytes())?
                .preview()
                .expect(expression)
                .after_bytes,
            expected.as_bytes()
        );
    }
    Ok(())
}

#[test]
fn split_negative_index_count_and_rstrip_character_set_match_python() -> Result<(), ActivityError> {
    for (expression, expected) in [
        ("s.split('é', 1)[-1].rstrip('é')", "b"),
        ("s.split('é', 0)[0]", "aébé"),
        ("s.split('é', -1)[-1]", ""),
        ("s.rstrip('éab')", ""),
        ("s.rstrip('')", "aébé"),
    ] {
        assert_eq!(
            transform(expression, "aébé".as_bytes())?
                .preview()
                .expect(expression)
                .after_bytes,
            expected.as_bytes()
        );
    }
    Ok(())
}

#[test]
fn substring_absence_and_empty_needle_start_beyond_length_refuse() -> Result<(), ActivityError> {
    for expression in ["s[s.index('missing'):]", "s[s.index('', 99):]"] {
        assert!(
            matches!(
                transform(expression, b"abc")?.preview(),
                Err(SnapshotPreviewError::SubstringNotFound { .. })
            ),
            "{expression}"
        );
    }
    assert_eq!(
        transform("s[s.index('', 3):]", b"abc")?
            .preview()
            .expect("empty needle at length")
            .after_bytes,
        b""
    );
    Ok(())
}

#[test]
fn split_item_outside_either_end_is_a_modeled_exception() -> Result<(), ActivityError> {
    for expression in ["s.split(':')[9]", "s.split(':')[-9]"] {
        assert!(
            matches!(
                transform(expression, b"a:b")?.preview(),
                Err(SnapshotPreviewError::SplitIndexOutOfRange { .. })
            ),
            "{expression}"
        );
    }
    Ok(())
}

#[test]
fn conditional_requires_predicate_and_selected_arm_snapshots_only() -> Result<(), ActivityError> {
    let source = "from pathlib import Path\na = Path('first').read_text(encoding='utf-8')\nb = Path('second').read_text(encoding='utf-8')\nPath('output').write_text(a + '!' if a.startswith('yes') else b, encoding='utf-8')";
    let mut fixture = Fixture::python(source, 0, None)?;
    fixture.read(0, 0, "first", b"yes", TextEncoding::Utf8);
    let preview = fixture.preview().expect("unused arm snapshot absent");
    assert_eq!(preview.after_bytes, b"yes!");
    assert_eq!(preview.read_bindings.len(), 1);
    fixture.input.reads[0].bytes = b"no".to_vec();
    assert!(matches!(
        fixture.preview(),
        Err(SnapshotPreviewError::MissingRead { .. })
    ));
    fixture.read(1, 0, "second", b"fallback", TextEncoding::Utf8);
    assert_eq!(
        fixture.preview().expect("selected alternative").after_bytes,
        b"fallback"
    );
    Ok(())
}

#[test]
fn endswith_predicate_uses_exact_unicode_suffix() -> Result<(), ActivityError> {
    let fixture = transform("'yes' if s.endswith('界') else 'no'", "é界".as_bytes())?;
    assert_eq!(
        fixture.preview().expect("suffix predicate").after_bytes,
        b"yes"
    );
    Ok(())
}

#[test]
fn no_newline_markers_and_utf8_range_boundaries_are_exact() -> Result<(), ActivityError> {
    let fixture = Fixture::python(
        "from pathlib import Path\nPath('output').write_text('ê尾', encoding='utf-8')",
        0,
        Some("é尾".as_bytes()),
    )?;
    let preview = fixture.preview().expect("Unicode endpoints");
    assert_eq!(preview.replacements[0].before.start, 0);
    assert_eq!(preview.replacements[0].before.end, "é".len());
    assert_eq!(preview.replacements[0].after.end, "ê".len());
    assert_eq!(
        preview
            .unified_diff
            .matches("\\ No newline at end of file")
            .count(),
        2
    );
    Ok(())
}

#[test]
fn empty_file_creation_is_distinct_from_unchanged_empty_file() -> Result<(), ActivityError> {
    let fixture = Fixture::python(
        "from pathlib import Path\nPath('output').write_text('', encoding='utf-8')",
        0,
        None,
    )?;
    let preview = fixture.preview().expect("empty creation");
    assert_eq!(preview.before_bytes, None);
    assert_eq!(preview.after_bytes, b"");
    assert_eq!(preview.replacements.len(), 0);
    assert_eq!(
        preview.unified_diff,
        "--- /dev/null\n+++ conditional-after\n"
    );
    Ok(())
}

#[test]
fn truncate_and_write_are_distinct_operation_epochs() -> Result<(), ActivityError> {
    let source = "open('output', 'w', encoding='utf-8').write('ok')";
    let first = Fixture::python(source, 0, Some(b"old"))?;
    let second = Fixture::python(source, 1, Some(b""))?;
    assert_eq!(first.preview().expect("truncate").after_bytes, b"");
    assert_eq!(second.preview().expect("write").after_bytes, b"ok");
    assert_eq!(second.input.before.binding.filesystem_epoch.get(), 1);
    Ok(())
}

#[test]
fn input_and_node_limits_are_enforced_independently() -> Result<(), ActivityError> {
    let mut fixture = transform("s + 'tail'", b"head")?;
    fixture.input.limits.input_bytes = 1;
    assert!(matches!(
        fixture.preview(),
        Err(SnapshotPreviewError::ResourceLimit {
            limit: SnapshotPreviewLimit::Input
        })
    ));
    fixture.input.limits.input_bytes = 4096;
    fixture.input.limits.nodes = 1;
    assert!(matches!(
        fixture.preview(),
        Err(SnapshotPreviewError::ResourceLimit {
            limit: SnapshotPreviewLimit::Nodes
        })
    ));
    Ok(())
}

#[test]
fn read_zero_from_one_child_source_cannot_satisfy_another_child() -> Result<(), ActivityError> {
    let body = "from pathlib import Path; s = Path('input').read_text(encoding='utf-8'); Path('output').write_text(s, encoding='utf-8')";
    let source = format!("python3 -c \"{body}\"\npython3 -c \"{body}\"");
    let mut fixture = Fixture::shell(&source, 1, None)?;
    fixture.read(0, 0, "input", b"right namespace", TextEncoding::Utf8);
    let correct_source = fixture.input.reads[0].binding.read.source_id;
    fixture.input.reads[0].binding.read.source_id = fixture.analysis.edits()[0].evidence.source_id;
    assert!(matches!(
        fixture.preview(),
        Err(SnapshotPreviewError::MissingRead { .. })
    ));
    fixture.input.reads[0].binding.read.source_id = correct_source;
    assert_eq!(
        fixture
            .preview()
            .expect("matched child namespace")
            .after_bytes,
        b"right namespace"
    );
    Ok(())
}
