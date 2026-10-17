//! Authored inert source analysis and supplied-byte materialization; no Python runs.

#[path = "selective_python_logical/bindings.rs"]
mod bindings;
#[path = "selective_python_logical/policies.rs"]
mod policies;
#[path = "selective_python_logical/support.rs"]
mod support;
#[path = "selective_python_logical/temporary.rs"]
mod temporary;

use codex_code_activity::PythonOperation;
use codex_code_activity::SelectiveEditDetails;
use codex_code_activity::SnapshotPreviewError;
use codex_code_activity::materialize_logical_stream_preview;
use pretty_assertions::assert_eq;
use support::TestResult;
use support::analyze;
use support::input;

#[test]
fn logical_endpoint_stops_later_reads_mutations_and_preserves_opaque_prefix() -> TestResult {
    for suffix in [
        "f.write('again')",
        "f.read()",
        "f.seek(0)",
        "unknown()",
        "open('x','w')",
    ] {
        let source = format!(
            "with open('out','r+',encoding='utf8',newline='') as f:\n    f.write('X')\n    {suffix}"
        );
        let analysis = analyze(&source)?;
        assert_eq!(analysis.edits().len(), 1, "{source}");
        assert_eq!(analysis.python_barriers().len(), 1, "{source}");
        let report = serde_json::to_value(&analysis)?;
        assert_eq!(report["edits"][0]["laterContext"], "opaqueSuffix");
    }
    let source = "with open('out','r+',encoding='utf8',newline='') as f:\n    f.write('X')\nopen('x','w').write('no')";
    assert_eq!(analyze(source)?.edits().len(), 1);
    Ok(())
}

#[test]
fn logical_stream_refuses_aliases_nonzero_cursor_and_unrewound_reads() -> TestResult {
    for body in [
        "g=f\n    g.write('X')",
        "f.seek(1)\n    f.write('X')",
        "s=f.read()\n    f.write('X')",
        "f.close()\n    f.write('X')",
        "f.read(1)\n    f.seek(0)\n    f.write('X')",
    ] {
        let analysis = analyze(&format!(
            "with open('out','r+',encoding='utf8',newline='') as f:\n    {body}"
        ))?;
        assert!(analysis.edits().is_empty(), "{body}");
        assert_eq!(analysis.python_barriers().len(), 1);
    }
    Ok(())
}

#[test]
fn logical_empty_payload_and_invalid_utf8_boundary_are_distinct() -> TestResult {
    let empty = analyze("with open('out','r+',encoding='utf8',newline='') as f: f.write('')")?;
    let preview =
        materialize_logical_stream_preview(&empty, 0, &input(&empty, 0, Some("éZ".as_bytes()))?)?;
    assert_eq!(preview.after_bytes, "éZ".as_bytes());
    assert!(preview.replacements.is_empty());
    let split = analyze("with open('out','r+',encoding='utf8',newline='') as f: f.write('X')")?;
    assert!(matches!(
        materialize_logical_stream_preview(&split, 0, &input(&split, 0, Some("éZ".as_bytes()))?),
        Err(SnapshotPreviewError::InvalidUtf8)
    ));
    Ok(())
}

#[test]
fn literal_short_circuit_skips_semantics_but_validates_complete_syntax() -> TestResult {
    for guard in ["False and unknown()", "True or unknown()"] {
        let source =
            format!("{guard}\nwith open('out','r+',encoding='utf8',newline='') as f: f.write('X')");
        let analysis = analyze(&source)?;
        assert!(analysis.python_barriers().is_empty());
        let SelectiveEditDetails::Python { prediction, .. } = &analysis.edits()[0].details else {
            return Err("not Python".into());
        };
        assert_eq!(prediction.predecessor_conditions.len(), 2);
        assert_eq!(
            &source[prediction.predecessor_conditions[0].start_byte
                ..prediction.predecessor_conditions[0].end_byte],
            guard
        );
        assert!(matches!(
            prediction.operation,
            PythonOperation::LogicalWrite(_)
        ));
    }
    for guard in [
        "True and unknown()",
        "False or unknown()",
        "False and (lambda: 1)",
        "False and broken(",
    ] {
        let analysis = analyze(&format!(
            "with open('out','r+',encoding='utf8',newline='') as f:\n    {guard}\n    f.write('X')"
        ))?;
        assert!(analysis.edits().is_empty(), "{guard}");
    }
    Ok(())
}
