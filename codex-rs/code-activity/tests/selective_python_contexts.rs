//! Authored inert Python sources; no context manager or file I/O is executed.

#[path = "selective_python_contexts/rejections.rs"]
mod rejections;
#[path = "selective_python_contexts/support.rs"]
mod support;

use codex_code_activity::SnapshotPreviewBasis;
use codex_code_activity::SnapshotPreviewScope;
use pretty_assertions::assert_eq;
use support::ReadSnapshot;
use support::TestResult;
use support::analyze;
use support::forecast;
use support::prediction;
use support::preview;
use support::text;

#[test]
fn implicit_close_is_the_write_point_with_exact_write_call_provenance() -> TestResult {
    let source = "with open('out', 'w', encoding='utf-8') as f:\n    f.write('é🧭\\n')";
    let analysis = analyze(source)?;
    let report = serde_json::to_value(&analysis)?;
    assert_eq!(report["disposition"], "analyzed");
    assert!(analysis.python_barriers().is_empty());
    assert_eq!(analysis.edits().len(), 2);
    assert_eq!(prediction(&report, 0)["operation"]["type"], "truncate");
    assert_eq!(
        text(source, &prediction(&report, 0)["evidence"])?,
        "open('out', 'w', encoding='utf-8')"
    );
    assert_eq!(text(source, &prediction(&report, 1)["evidence"])?, source);
    let artifact = preview(&analysis, 1, Some(b""), None)?;
    assert_eq!(
        artifact.basis,
        SnapshotPreviewBasis::SuppliedSnapshotConditional
    );
    assert_eq!(artifact.scope, SnapshotPreviewScope::OperationPoint);
    assert_eq!(artifact.after_bytes, "é🧭\n".as_bytes());
    assert_eq!(
        artifact.unified_diff,
        "--- supplied-before\n+++ conditional-after\n@@ -0,0 +1,1 @@\n+é🧭\n"
    );
    let replacement = artifact.replacements[0];
    assert_eq!(replacement.before.start, 0);
    assert_eq!(replacement.before.end, 0);
    assert_eq!(replacement.after.end, "é🧭\n".len());
    let conditions = prediction(&report, 1)["predecessorConditions"]
        .as_array()
        .ok_or("missing write conditions")?;
    assert_eq!(
        conditions
            .iter()
            .map(|span| text(source, span))
            .collect::<TestResult<Vec<_>>>()?,
        ["open('out', 'w', encoding='utf-8')", "f.write('é🧭\\n')"]
    );
    assert_eq!(prediction(&report, 1)["finalState"], "notEstablished");
    Ok(())
}

#[test]
fn read_context_captures_text_and_normal_close_before_later_rewrite() -> TestResult {
    let source = "with open('input', encoding='utf8') as f:\n    text = f.read()\nsaved = text\ntext = 'unused'\nwith open('out', 'w', encoding='utf-8') as f:\n    f.write(saved.replace('old', '新'))";
    let analysis = analyze(source)?;
    assert!(analysis.python_barriers().is_empty());
    assert_eq!(analysis.edits().len(), 2);
    let artifact = preview(
        &analysis,
        1,
        Some(b""),
        Some(ReadSnapshot {
            id: 0,
            epoch: 0,
            path: "input",
            bytes: b"old\r\nold\r",
        }),
    )?;
    assert_eq!(artifact.after_bytes, "新\n新\n".as_bytes());
    let report = serde_json::to_value(&analysis)?;
    let recipe = &prediction(&report, 1)["operation"]["content"];
    assert_eq!(recipe["input"]["readId"], 0);
    assert_eq!(recipe["input"]["filesystemEpoch"], 0);
    assert_eq!(text(source, &recipe["input"]["evidence"])?, "f.read()");
    let conditions = prediction(&report, 0)["predecessorConditions"]
        .as_array()
        .ok_or("missing read-close conditions")?;
    assert_eq!(
        text(
            source,
            conditions.last().ok_or("missing implicit read close")?
        )?,
        "with open('input', encoding='utf8') as f:\n    text = f.read()"
    );
    Ok(())
}

#[test]
fn pending_write_keeps_target_content_and_owner_binding_versions() -> TestResult {
    let source = "path='out'\ncontent='captured\\n'\nwith open(path, 'w', encoding='utf-8') as f:\n    f.write(content)\n    path='other'\n    content='later'";
    let analysis = analyze(source)?;
    let report = serde_json::to_value(&analysis)?;
    let output = prediction(&report, 1);
    assert_eq!(output["target"]["path"], "out");
    assert_eq!(
        preview(&analysis, 1, Some(b""), None)?.after_bytes,
        b"captured\n"
    );
    let bindings = output["dependencies"]
        .as_array()
        .ok_or("missing dependencies")?
        .iter()
        .filter(|dependency| dependency["type"] == "binding")
        .map(|dependency| {
            Ok((
                dependency["name"].as_str().ok_or("missing binding name")?,
                dependency["version"]
                    .as_u64()
                    .ok_or("missing binding version")?,
                text(source, &dependency["evidence"])?,
            ))
        })
        .collect::<TestResult<Vec<_>>>()?;
    assert_eq!(
        bindings,
        [
            ("path", 1, "path='out'"),
            ("f", 3, "open(path, 'w', encoding='utf-8') as f"),
            ("content", 2, "content='captured\\n'"),
        ]
    );
    Ok(())
}

#[test]
fn explicit_close_flushes_once_and_later_opaque_calls_do_not_erase_it() -> TestResult {
    let source = "with open('out', 'w', encoding='utf-8') as f:\n    f.write('ready')\n    f.close()\n    f.close()\nopaque()";
    let report = forecast(source)?;
    assert_eq!(report["edits"].as_array().ok_or("missing edits")?.len(), 2);
    assert_eq!(
        text(source, &prediction(&report, 1)["evidence"])?,
        "f.close()"
    );
    assert_eq!(prediction(&report, 1)["finalState"], "opaqueSuccessor");
    assert_eq!(
        prediction(&report, 1)["operation"]["content"]["value"],
        "ready"
    );
    Ok(())
}

#[test]
fn zero_writes_only_truncate_and_empty_write_is_still_a_close_point() -> TestResult {
    for body in ["pass", "f.close()"] {
        let report = forecast(&format!(
            "with open('out', 'w', encoding='utf-8') as f:\n    {body}"
        ))?;
        assert_eq!(report["edits"].as_array().ok_or("missing edits")?.len(), 1);
        assert_eq!(prediction(&report, 0)["operation"]["type"], "truncate");
    }
    let analysis = analyze("with open('out', 'w', encoding='utf-8') as f: f.write('')")?;
    assert_eq!(analysis.edits().len(), 2);
    let artifact = preview(&analysis, 1, Some(b""), None)?;
    assert!(artifact.after_bytes.is_empty());
    assert!(artifact.unified_diff.is_empty());
    Ok(())
}

#[test]
fn read_after_closed_write_uses_the_close_mutation_epoch() -> TestResult {
    let source = "with open('first', 'w', encoding='utf-8') as f:\n    f.write('new')\nwith open('first', 'r', encoding='utf-8') as f:\n    text=f.read()\nwith open('out', 'w', encoding='utf-8') as f:\n    f.write(text)";
    let analysis = analyze(source)?;
    assert!(analysis.python_barriers().is_empty());
    assert_eq!(analysis.edits().len(), 4);
    let artifact = preview(
        &analysis,
        3,
        Some(b""),
        Some(ReadSnapshot {
            id: 0,
            epoch: 2,
            path: "first",
            bytes: b"new",
        }),
    )?;
    assert_eq!(artifact.after_bytes, b"new");
    let report = serde_json::to_value(&analysis)?;
    assert_eq!(
        prediction(&report, 3)["operation"]["content"]["filesystemEpoch"],
        2
    );
    Ok(())
}

#[test]
fn modeled_close_errors_remain_conditional_premises_for_successors() -> TestResult {
    let source = "with open('first', 'w', encoding='utf-8') as f:\n    f.write('first')\nopen('later', 'w', encoding='utf-8').write('later')";
    let report = forecast(source)?;
    let conditions = prediction(&report, 2)["predecessorConditions"]
        .as_array()
        .ok_or("missing close predecessor")?;
    assert_eq!(
        text(source, conditions.last().ok_or("missing close premise")?)?,
        "with open('first', 'w', encoding='utf-8') as f:\n    f.write('first')"
    );
    assert_eq!(report["edits"][1]["basis"], "standardApiForecast");
    assert_eq!(
        prediction(&report, 1)["finalState"],
        "possibleAliasLaterTouch"
    );
    Ok(())
}

#[test]
fn parenthesized_context_and_literal_path_composition_keep_one_owned_handle() -> TestResult {
    let source = "name='out'\nwith (open('dir/' + name, mode='w', encoding='utf8') as f,):\n    f.write('parenthesized')";
    let analysis = analyze(source)?;
    assert!(analysis.python_barriers().is_empty());
    assert_eq!(analysis.edits().len(), 2);
    let report = serde_json::to_value(&analysis)?;
    assert_eq!(prediction(&report, 1)["target"]["path"], "dir/out");
    assert_eq!(
        preview(&analysis, 1, Some(b""), None)?.after_bytes,
        b"parenthesized"
    );
    Ok(())
}

#[test]
fn read_only_contexts_are_nonmutating_and_closed_owner_rebinding_is_local() -> TestResult {
    let read_only = analyze("with open('input', encoding='utf-8') as f:\n    text=f.read()")?;
    assert!(read_only.python_barriers().is_empty());
    assert!(read_only.edits().is_empty());
    let source = "with open('out', 'w', encoding='utf-8') as f:\n    f.write('closed')\n    f.close()\n    f='rebound'";
    let analysis = analyze(source)?;
    assert!(analysis.python_barriers().is_empty());
    assert_eq!(analysis.edits().len(), 2);
    assert_eq!(
        preview(&analysis, 1, Some(b""), None)?.after_bytes,
        b"closed"
    );
    Ok(())
}
