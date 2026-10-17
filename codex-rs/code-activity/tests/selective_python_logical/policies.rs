use super::support::TestResult;
use super::support::analyze;
use super::support::input;
use codex_code_activity::PythonStreamPosition;
use codex_code_activity::SnapshotEpoch;
use codex_code_activity::SnapshotPreviewError;
use codex_code_activity::SnapshotReadBinding;
use codex_code_activity::SnapshotReadNewline;
use codex_code_activity::SnapshotWriteNewline;
use codex_code_activity::SourceReadId;
use codex_code_activity::Target;
use codex_code_activity::TextEncoding;
use codex_code_activity::TrustedReadSnapshot;
use codex_code_activity::materialize_logical_stream_preview;
use pretty_assertions::assert_eq;

#[test]
fn logical_rewind_requires_exact_read_epoch_and_retains_byte_suffix() -> TestResult {
    let source = "with open('out','r+',encoding='utf8',newline='') as f:\n    s=f.read()\n    f.seek(0)\n    f.write(s.replace('go','X',1))";
    let analysis = analyze(source)?;
    assert!(analysis.python_barriers().is_empty());
    let mut input = input(&analysis, 0, Some(b"gogo"))?;
    let id = SourceReadId::new(analysis.edits()[0].evidence.source_id, 0);
    let binding = SnapshotReadBinding::new(
        id,
        SnapshotEpoch::new(0),
        Target::Literal {
            path: "out".into(),
            cwd: None,
        },
        TextEncoding::Utf8,
        SnapshotReadNewline::Preserve,
    );
    input
        .snapshot
        .reads
        .push(TrustedReadSnapshot::new(binding, b"gogo".to_vec()));
    let preview = materialize_logical_stream_preview(&analysis, 0, &input)?;
    assert_eq!(preview.after_bytes, b"Xgoo");
    assert!(matches!(
        preview.logical_stream.ok_or("missing metadata")?.position,
        PythonStreamPosition::RewoundZero { read_id: 0, .. }
    ));
    input.snapshot.reads[0].binding.filesystem_epoch = SnapshotEpoch::new(1);
    assert!(matches!(
        materialize_logical_stream_preview(&analysis, 0, &input),
        Err(SnapshotPreviewError::ReadBindingMismatch { .. })
    ));
    input.snapshot.reads[0].binding.filesystem_epoch = SnapshotEpoch::new(0);
    input.snapshot.reads[0].bytes = b"diff".to_vec();
    assert!(matches!(
        materialize_logical_stream_preview(&analysis, 0, &input),
        Err(SnapshotPreviewError::ConflictingSnapshots { .. })
    ));
    Ok(())
}

#[test]
fn logical_mixed_read_policies_are_source_bound_and_old_profile_stays_strict() -> TestResult {
    let source = "with open('a',encoding='utf8',newline=None) as a:\n    x=a.read()\nwith open('b',encoding='utf8',newline='') as b:\n    y=b.read()\nwith open('out','w',encoding='utf8',newline='') as f:\n    f.write(x+y)";
    let analysis = analyze(source)?;
    assert!(analysis.python_barriers().is_empty());
    let mut input = input(&analysis, 1, Some(b""))?;
    let source_id = analysis.edits()[1].evidence.source_id;
    for (read_id, path, newline) in [
        (0, "a", SnapshotReadNewline::PythonUniversal),
        (1, "b", SnapshotReadNewline::Preserve),
    ] {
        input.snapshot.reads.push(TrustedReadSnapshot::new(
            SnapshotReadBinding::new(
                SourceReadId::new(source_id, read_id),
                SnapshotEpoch::new(0),
                Target::Literal {
                    path: path.into(),
                    cwd: None,
                },
                TextEncoding::Utf8,
                newline,
            ),
            b"\r\n".to_vec(),
        ));
    }
    let preview = materialize_logical_stream_preview(&analysis, 1, &input)?;
    assert_eq!(preview.after_bytes, b"\n\r\n");
    assert_eq!(preview.read_bindings.len(), 2);
    assert_eq!(
        preview
            .logical_stream
            .ok_or("missing metadata")?
            .binding
            .stream_id
            .get(),
        2
    );
    input.snapshot.text_profile.read_newline = SnapshotReadNewline::PythonUniversal;
    assert!(matches!(
        materialize_logical_stream_preview(&analysis, 1, &input),
        Err(SnapshotPreviewError::NewlineProfileMismatch)
    ));
    input.snapshot.text_profile.read_newline = SnapshotReadNewline::SourceDeclared;
    input.snapshot.reads[1].binding.newline = SnapshotReadNewline::SourceDeclared;
    assert!(matches!(
        materialize_logical_stream_preview(&analysis, 1, &input),
        Err(SnapshotPreviewError::ReadBindingMismatch { .. })
    ));
    Ok(())
}

#[test]
fn logical_output_translation_preserves_pretranslation_character_count() -> TestResult {
    for (literal, profile, after) in [
        ("None", SnapshotWriteNewline::CrLf, "é\r\n\r\r\n"),
        ("'\\r'", SnapshotWriteNewline::Cr, "é\r\r\r"),
        ("'\\r\\n'", SnapshotWriteNewline::CrLf, "é\r\n\r\r\n"),
        ("'\\n'", SnapshotWriteNewline::Preserve, "é\n\r\n"),
    ] {
        let analysis = analyze(&format!(
            "with open('out','w',encoding='utf8',newline={literal}) as f: f.write('é\\n\\r\\n')"
        ))?;
        let mut input = input(&analysis, 1, Some(b""))?;
        input.snapshot.text_profile.write_newline = profile;
        let preview = materialize_logical_stream_preview(&analysis, 1, &input)?;
        assert_eq!(preview.after_bytes, after.as_bytes());
        assert_eq!(
            preview
                .logical_stream
                .ok_or("missing metadata")?
                .accepted_characters_required,
            4
        );
        input.snapshot.text_profile.write_newline = SnapshotWriteNewline::Unspecified;
        assert!(matches!(
            materialize_logical_stream_preview(&analysis, 1, &input),
            Err(SnapshotPreviewError::NewlineProfileMismatch)
        ));
    }
    Ok(())
}

#[test]
fn logical_legacy_omission_and_pathlib_policy_are_distinct_source_contracts() -> TestResult {
    let legacy = analyze("with open('out','w',encoding='utf8') as f: f.write('X')")?;
    let report = serde_json::to_value(&legacy)?;
    assert_eq!(
        report["edits"][1]["details"]["prediction"]["operation"]["type"],
        "writeText"
    );
    assert!(
        report["edits"][1]["details"]["prediction"]["operation"]
            .get("newline")
            .is_none()
    );
    let path = analyze(
        "from pathlib import Path\nx=Path('a').read_text(encoding='utf8',newline='')\nPath('out').write_text(x,encoding='utf8',newline='\\r')",
    )?;
    assert!(path.python_barriers().is_empty());
    let report = serde_json::to_value(&path)?;
    let operation = &report["edits"][0]["details"]["prediction"]["operation"];
    assert_eq!(operation["newline"], "cr");
    assert_eq!(operation["content"]["newline"], "preserve");
    assert!(
        report["edits"][0]["details"]["assumptions"]
            .as_array()
            .ok_or("missing assumptions")?
            .iter()
            .any(|value| value == "pathlibNewlineSupport")
    );
    Ok(())
}
