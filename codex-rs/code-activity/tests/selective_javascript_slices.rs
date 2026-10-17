//! Captured Node slices are described only; no JavaScript runtime is invoked.

#[path = "selective_node_capabilities/support.rs"]
mod support;

use pretty_assertions::assert_eq;
use support::TestResult;
use support::bindings;
use support::dependencies;
use support::edits;
use support::forecast;
use support::prediction;
use support::span_text;

#[test]
fn slice_freezes_receiver_and_integer_bounds_before_later_rebindings() -> TestResult {
    let source = "const fs=require('fs'); let text=fs.readFileSync('input','utf8'); let start=0; let end=2; const captured=text.slice(start,end); start=1; end=9; text='changed'; fs.writeFileSync('out',captured);";
    let report = forecast(source)?;
    assert_eq!(edits(&report)?.len(), 1);
    assert!(report.get("javascriptBarriers").is_none());
    let edit = prediction(&report, 0);
    let content = &edit["content"];
    assert_eq!(content["type"], "slice");
    assert_eq!(content["start"], 0);
    assert_eq!(content["end"], 2);
    assert_eq!(
        span_text(source, &content["evidence"])?,
        "text.slice(start,end)"
    );
    assert_eq!(content["input"]["type"], "readUtf8");
    assert_eq!(content["input"]["readId"], 0);
    assert_eq!(content["input"]["filesystemEpoch"], 0);
    assert_eq!(
        bindings(edit)?,
        [
            ("captured", 5),
            ("end", 4),
            ("fs", 1),
            ("start", 3),
            ("text", 2)
        ]
    );
    assert!(dependencies(edit)?.iter().any(|dependency| {
        dependency["type"] == "capability" && dependency["assumption"] == "standardStringMethods"
    }));
    assert_eq!(
        edit["predecessorConditions"].as_array().map(Vec::len),
        Some(2)
    );
    Ok(())
}

#[test]
fn slice_distinguishes_omitted_end_from_a_captured_explicit_end() -> TestResult {
    for (arguments, end) in [("1", None), ("1,3", Some(3))] {
        let report = forecast(&format!(
            "const fs=require('fs'); fs.writeFileSync('out','abc'.slice({arguments}));"
        ))?;
        assert_eq!(edits(&report)?.len(), 1);
        assert_eq!(prediction(&report, 0)["content"]["start"], 1);
        assert_eq!(
            prediction(&report, 0)["content"]["end"],
            serde_json::to_value(end)?
        );
    }
    Ok(())
}

#[test]
fn slice_read_keeps_source_local_epoch_after_a_path_overwrite() -> TestResult {
    let report = forecast(
        "const fs=require('fs'); const saved=fs.readFileSync('input','utf8'); fs.writeFileSync('input','changed'); fs.writeFileSync('out',saved.slice(0,2));",
    )?;
    assert_eq!(edits(&report)?.len(), 2);
    let edit = prediction(&report, 1);
    assert_eq!(edit["content"]["input"]["filesystemEpoch"], 0);
    for epoch in [0, 1] {
        assert!(dependencies(edit)?.iter().any(|dependency| {
            dependency["type"] == "filesystemEpoch" && dependency["epoch"] == epoch
        }));
    }
    Ok(())
}

#[test]
fn noninteger_bounds_and_wrong_arity_stop_only_successors() -> TestResult {
    for arguments in ["", "0,1,2", "'0'", "null", "unknown", "0,'2'", "0,null"] {
        let report = forecast(&format!(
            "const fs=require('fs'); fs.writeFileSync('before','x'); fs.writeFileSync('bad','abc'.slice({arguments})); fs.writeFileSync('after','y');"
        ))?;
        assert_eq!(edits(&report)?.len(), 1, "{arguments}");
        assert_eq!(prediction(&report, 0)["target"]["path"], "before");
        assert_eq!(report["edits"][0]["laterContext"], "opaqueSuffix");
        assert_eq!(
            report["javascriptBarriers"].as_array().map(Vec::len),
            Some(1)
        );
    }
    Ok(())
}

#[test]
fn unsupported_numeric_syntax_still_refuses_the_complete_source() -> TestResult {
    for bound in ["-1", "1.5", "1e2", "0x10", "01", "1n", "9007199254740992"] {
        let report = forecast(&format!(
            "const fs=require('fs'); fs.writeFileSync('before','x'); fs.writeFileSync('after','abc'.slice({bound}));"
        ))?;
        assert!(edits(&report)?.is_empty(), "{bound}");
        assert_eq!(report["disposition"]["unavailable"], "unsupportedSource");
    }
    Ok(())
}

#[test]
fn slice_does_not_cross_unknown_calls_or_capture_string_method_aliases() -> TestResult {
    for statement in ["unknown();", "const slice='abc'.slice;"] {
        let report = forecast(&format!(
            "const fs=require('fs'); fs.writeFileSync('before','x'); {statement} fs.writeFileSync('after','abc'.slice(0,2));"
        ))?;
        assert_eq!(edits(&report)?.len(), 1, "{statement}");
        assert_eq!(prediction(&report, 0)["target"]["path"], "before");
        assert_eq!(report["edits"][0]["laterContext"], "opaqueSuffix");
    }
    Ok(())
}

#[test]
fn deferred_slice_byte_length_does_not_invent_a_later_descriptor_cursor() -> TestResult {
    let report = forecast(
        "const fs=require('fs'); const text=fs.readFileSync('input','utf8'); const fd=fs.openSync('out','r+'); fs.writeSync(fd,text.slice(0,2)); fs.writeSync(fd,'X',9); fs.writeSync(fd,'Y'); fs.writeFileSync('after','wrong');",
    )?;
    assert_eq!(edits(&report)?.len(), 2);
    assert_eq!(prediction(&report, 0)["content"]["type"], "slice");
    assert_eq!(prediction(&report, 0)["writeEffect"]["byteOffset"], 0);
    assert_eq!(prediction(&report, 1)["writeEffect"]["byteOffset"], 9);
    assert_eq!(
        report["javascriptBarriers"][0]["reason"],
        "unsupportedOperation"
    );
    assert_eq!(report["edits"][1]["laterContext"], "opaqueSuffix");
    Ok(())
}

#[test]
fn surrogate_split_is_a_typed_recipe_not_an_exact_utf8_preview() -> TestResult {
    let report = forecast("require('fs').writeFileSync('out','😀a'.slice(1));")?;
    assert_eq!(edits(&report)?.len(), 1);
    let content = &prediction(&report, 0)["content"];
    assert_eq!(content["type"], "slice");
    assert_eq!(content["start"], 1);
    assert!(content["end"].is_null());
    assert_eq!(content["input"]["type"], "literal");
    assert!(report.get("javascriptBarriers").is_none());
    Ok(())
}

#[test]
fn surrogate_recipe_does_not_erase_prefix_or_cross_an_unknown_successor() -> TestResult {
    let report = forecast(
        "const fs=require('fs'); fs.writeFileSync('before','ok'); fs.writeFileSync('out','😀a'.slice(1)); unknown(); fs.writeFileSync('after','wrong');",
    )?;
    assert_eq!(edits(&report)?.len(), 2);
    assert_eq!(prediction(&report, 0)["target"]["path"], "before");
    assert_eq!(prediction(&report, 1)["content"]["type"], "slice");
    assert_eq!(report["edits"][1]["laterContext"], "opaqueSuffix");
    assert_eq!(
        report["javascriptBarriers"].as_array().map(Vec::len),
        Some(1)
    );
    Ok(())
}
