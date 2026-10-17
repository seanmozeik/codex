//! Endpoint compatibility over authored inert source; no Python is executed.

use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::Language;
use codex_code_activity::PythonWriteEndpoint;
use codex_code_activity::SelectiveEditAnalysis;
use codex_code_activity::SelectiveEditInput;
use codex_code_activity::SnapshotEpoch;
use codex_code_activity::SnapshotPreviewError;
use codex_code_activity::SnapshotPreviewInput;
use codex_code_activity::SnapshotPreviewScope;
use codex_code_activity::SnapshotReadBinding;
use codex_code_activity::SnapshotReadNewline;
use codex_code_activity::SnapshotTargetBinding;
use codex_code_activity::SnapshotTextProfile;
use codex_code_activity::SnapshotWriteNewline;
use codex_code_activity::SourceReadId;
use codex_code_activity::Target;
use codex_code_activity::TextEncoding;
use codex_code_activity::TrustedReadSnapshot;
use codex_code_activity::TrustedTargetSnapshot;
use codex_code_activity::materialize_snapshot_preview;
use pretty_assertions::assert_eq;
use serde_json::Value;
use std::time::Duration;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn analyze(
    source: &str,
    language: Language,
    endpoint: PythonWriteEndpoint,
) -> TestResult<SelectiveEditAnalysis> {
    let input = SelectiveEditInput::from_source(
        "endpoint-test".into(),
        language,
        source,
        None,
        AnalysisLimits::new(32_768, Duration::from_secs(2)),
    )?
    .with_python_write_endpoint(endpoint);
    Ok(Analyzer::new()?.forecast_edits(input))
}

fn source_text<'a>(source: &'a str, span: &Value) -> TestResult<&'a str> {
    let start = usize::try_from(span["startByte"].as_u64().ok_or("missing start")?)?;
    let end = usize::try_from(span["endByte"].as_u64().ok_or("missing end")?)?;
    source.get(start..end).ok_or_else(|| "invalid span".into())
}

fn snapshot(analysis: &SelectiveEditAnalysis) -> TestResult<SnapshotPreviewInput> {
    let source_id = analysis.sources().first().ok_or("missing source")?.id;
    Ok(SnapshotPreviewInput::new(
        analysis.call_id().clone(),
        TrustedTargetSnapshot::new(
            SnapshotTargetBinding::new(
                source_id,
                SnapshotEpoch::new(1),
                Target::Literal {
                    path: "out".into(),
                    cwd: None,
                },
            ),
            Some(Vec::new()),
        ),
    ))
}

#[test]
fn close_view_preserves_explicit_newlines_and_write_predecessors() -> TestResult {
    for (newline, profile, expected) in [
        ("''", SnapshotWriteNewline::Preserve, "a\r\nb\n"),
        ("'\\r'", SnapshotWriteNewline::Cr, "a\r\rb\r"),
        ("'\\r\\n'", SnapshotWriteNewline::CrLf, "a\r\r\nb\r\n"),
        ("None", SnapshotWriteNewline::CrLf, "a\r\r\nb\r\n"),
    ] {
        let source = format!(
            "with open('out','w',encoding='utf8',newline={newline}) as f:\n    f.write('a\\r\\nb\\n')"
        );
        let analysis = analyze(
            &source,
            Language::Python,
            PythonWriteEndpoint::ContextCloseWhenAvailable,
        )?;
        assert!(analysis.python_barriers().is_empty());
        assert_eq!(analysis.edits().len(), 2);
        let json = serde_json::to_value(&analysis)?;
        let write = &json["edits"][1]["details"]["prediction"];
        assert_eq!(write["operation"]["type"], "writeText");
        assert_eq!(source_text(&source, &write["evidence"])?, source);
        let conditions = write["predecessorConditions"]
            .as_array()
            .ok_or("missing conditions")?;
        assert!(
            conditions
                .iter()
                .any(|span| { source_text(&source, span).ok() == Some("f.write('a\\r\\nb\\n')") })
        );
        assert!(!conditions.contains(&write["evidence"]));
        let mut input = snapshot(&analysis)?;
        input.text_profile = SnapshotTextProfile::new(SnapshotReadNewline::SourceDeclared, profile);
        let preview = materialize_snapshot_preview(&analysis, 1, &input)?;
        assert_eq!(preview.after_bytes, expected.as_bytes());
        assert_eq!(preview.scope, SnapshotPreviewScope::OperationPoint);
        assert!(preview.logical_stream.is_none());
        input.text_profile.write_newline = SnapshotWriteNewline::Unspecified;
        assert!(matches!(
            materialize_snapshot_preview(&analysis, 1, &input),
            Err(SnapshotPreviewError::NewlineProfileMismatch)
        ));
    }
    Ok(())
}

#[test]
fn logical_view_remains_at_write_and_cannot_use_physical_binding() -> TestResult {
    let source = "with open('out','w',encoding='utf8',newline='') as f: f.write('X')";
    let analysis = analyze(
        source,
        Language::Python,
        PythonWriteEndpoint::LogicalAcceptance,
    )?;
    let json = serde_json::to_value(&analysis)?;
    let write = &json["edits"][1]["details"]["prediction"];
    assert_eq!(analysis.edits().len(), 2);
    assert_eq!(write["operation"]["type"], "logicalWrite");
    assert_eq!(source_text(source, &write["evidence"])?, "f.write('X')");
    assert!(
        !write["predecessorConditions"]
            .as_array()
            .ok_or("missing conditions")?
            .contains(&write["evidence"])
    );
    assert!(matches!(
        materialize_snapshot_preview(&analysis, 1, &snapshot(&analysis)?),
        Err(SnapshotPreviewError::UnsupportedOperation)
    ));
    Ok(())
}

#[test]
fn close_view_retains_the_read_epoch_and_exact_translated_replacement() -> TestResult {
    for (newline, profile, expected) in [
        ("''", SnapshotWriteNewline::Preserve, "a\r\nB\n"),
        ("'\\r\\n'", SnapshotWriteNewline::CrLf, "a\r\r\nB\r\n"),
    ] {
        let source = format!(
            "with open('out','r',encoding='utf8',newline='') as f:\n    s=f.read()\nwith open('out','w',encoding='utf8',newline={newline}) as f:\n    f.write(s.replace('b','B'))"
        );
        let analysis = analyze(
            &source,
            Language::Python,
            PythonWriteEndpoint::ContextCloseWhenAvailable,
        )?;
        let mut input = snapshot(&analysis)?;
        input.text_profile = SnapshotTextProfile::new(SnapshotReadNewline::SourceDeclared, profile);
        input.reads.push(TrustedReadSnapshot::new(
            SnapshotReadBinding::new(
                SourceReadId::new(analysis.sources()[0].id, 0),
                SnapshotEpoch::new(0),
                Target::Literal {
                    path: "out".into(),
                    cwd: None,
                },
                TextEncoding::Utf8,
                SnapshotReadNewline::Preserve,
            ),
            b"a\r\nb\n".to_vec(),
        ));
        assert_eq!(
            materialize_snapshot_preview(&analysis, 1, &input)?.after_bytes,
            expected.as_bytes()
        );
        input.reads[0].binding.filesystem_epoch = SnapshotEpoch::new(1);
        assert!(matches!(
            materialize_snapshot_preview(&analysis, 1, &input),
            Err(SnapshotPreviewError::ReadBindingMismatch { .. })
        ));
    }
    Ok(())
}

#[test]
fn update_mode_prefix_is_retained_in_both_views_without_inventing_close_bytes() -> TestResult {
    let source =
        "with open('out','r+',encoding='utf8',newline='') as f:\n    f.write('X')\n    opaque()";
    let logical = analyze(
        source,
        Language::Python,
        PythonWriteEndpoint::LogicalAcceptance,
    )?;
    let close = analyze(
        source,
        Language::Python,
        PythonWriteEndpoint::ContextCloseWhenAvailable,
    )?;
    assert_eq!(
        serde_json::to_value(&logical)?,
        serde_json::to_value(&close)?
    );
    assert_eq!(close.edits().len(), 1);
    assert_eq!(close.python_barriers().len(), 1);
    Ok(())
}

#[test]
fn endpoint_choice_reaches_owned_shell_children() -> TestResult {
    let python = "with open('out','w',encoding='utf8',newline='') as f: f.write('X')";
    let source = format!("python3 - <<'PY'\n{python}\nPY");
    let analysis = analyze(
        &source,
        Language::Shell,
        PythonWriteEndpoint::ContextCloseWhenAvailable,
    )?;
    let json = serde_json::to_value(&analysis)?;
    assert_eq!(analysis.edits().len(), 2);
    let write = &json["edits"][1]["details"]["prediction"];
    assert_eq!(write["operation"]["type"], "writeText");
    assert_eq!(write["evidence"]["sourceId"], 1);
    assert_eq!(source_text(python, &write["evidence"])?, python);
    Ok(())
}

#[test]
fn opaque_in_context_does_not_establish_close_but_default_keeps_logical_write() -> TestResult {
    let source =
        "with open('out','w',encoding='utf8',newline='') as f:\n    f.write('X')\n    opaque()";
    let close = analyze(
        source,
        Language::Python,
        PythonWriteEndpoint::ContextCloseWhenAvailable,
    )?;
    assert_eq!(close.edits().len(), 1);
    assert_eq!(close.python_barriers().len(), 1);
    let logical = analyze(source, Language::Python, PythonWriteEndpoint::default())?;
    assert_eq!(logical.edits().len(), 2);
    assert_eq!(logical.python_barriers().len(), 1);
    Ok(())
}
