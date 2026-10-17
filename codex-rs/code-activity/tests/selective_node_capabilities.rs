//! Authored source-only capability snapshots; JavaScript is never executed.

#[path = "selective_node_capabilities/descriptors.rs"]
mod descriptors;
#[path = "selective_node_capabilities/rejections.rs"]
mod rejections;
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
fn named_method_declarations_keep_bindings_read_evidence_and_premises() -> TestResult {
    for (module, declaration) in [("fs", "const"), ("node:fs", "let")] {
        let source = format!(
            "{declaration} fs=require('{module}'); {declaration} read=fs.readFileSync; {declaration} write=fs.writeFileSync; const text=read('input','utf8'); write('output',text.replace('old','new'),'utf8');"
        );
        let report = forecast(&source)?;
        assert_eq!(report["disposition"], "analyzed");
        assert_eq!(edits(&report)?.len(), 1);
        assert!(report.get("javascriptBarriers").is_none());
        let edit = prediction(&report, 0);
        assert_eq!(edit["target"]["path"], "output");
        assert_eq!(edit["target"]["cwd"], "/authored");
        assert_eq!(edit["content"]["type"], "replaceFirst");
        assert_eq!(edit["content"]["input"]["type"], "readUtf8");
        assert_eq!(edit["content"]["input"]["filesystemEpoch"], 0);
        assert_eq!(
            bindings(edit)?,
            [("fs", 1), ("read", 2), ("text", 4), ("write", 3)]
        );
        assert_eq!(
            span_text(&source, &edit["content"]["input"]["evidence"])?,
            "read('input','utf8')"
        );
        assert_eq!(
            span_text(&source, &edit["evidence"])?,
            "write('output',text.replace('old','new'),'utf8')"
        );
        for (name, initializer) in [("read", "fs.readFileSync"), ("write", "fs.writeFileSync")] {
            let binding = dependencies(edit)?
                .iter()
                .find(|value| value["type"] == "binding" && value["name"] == name)
                .ok_or("missing capability binding")?;
            assert_eq!(
                span_text(&source, &binding["evidence"])?,
                format!("{declaration} {name}={initializer};")
            );
        }
        let conditions = edit["predecessorConditions"]
            .as_array()
            .ok_or("missing predecessor conditions")?;
        assert_eq!(conditions.len(), 2);
        assert_eq!(
            span_text(&source, &conditions[0])?,
            format!("require('{module}')")
        );
        assert_eq!(span_text(&source, &conditions[1])?, "read('input','utf8')");
        for assumption in [
            "standardNodeCommonJsEvaluation",
            "canonicalNodeFs",
            "standardStringMethods",
            "noConcurrentMutation",
        ] {
            assert!(dependencies(edit)?.iter().any(|dependency| {
                dependency["type"] == "capability" && dependency["assumption"] == assumption
            }));
        }
    }
    Ok(())
}

#[test]
fn direct_require_member_snapshot_keeps_module_load_condition() -> TestResult {
    let source = "const write=(require('node:fs').writeFileSync); write('out','text');";
    let report = forecast(source)?;
    assert_eq!(edits(&report)?.len(), 1);
    let edit = prediction(&report, 0);
    assert_eq!(edit["content"]["value"], "text");
    assert_eq!(bindings(edit)?, [("write", 1)]);
    let conditions = edit["predecessorConditions"]
        .as_array()
        .ok_or("missing predecessor conditions")?;
    assert_eq!(conditions.len(), 1);
    assert_eq!(span_text(source, &conditions[0])?, "require('node:fs')");
    Ok(())
}

#[test]
fn onward_destructured_function_aliases_use_the_same_read_and_write_dispatch() -> TestResult {
    for declaration in ["const", "let"] {
        let source = format!(
            "const {{readFileSync:read,writeFileSync:write}}=require('fs'); {declaration} savedRead=read; {declaration} savedWrite=write; const text=savedRead('input','utf8'); savedWrite('out',text);"
        );
        let report = forecast(&source)?;
        assert_eq!(edits(&report)?.len(), 1);
        let edit = prediction(&report, 0);
        assert_eq!(edit["content"]["type"], "readUtf8");
        assert_eq!(
            bindings(edit)?,
            [
                ("read", 1),
                ("savedRead", 3),
                ("savedWrite", 4),
                ("text", 5),
                ("write", 2),
            ]
        );
    }
    Ok(())
}

#[test]
fn object_and_method_snapshots_keep_original_versions_after_object_rebindings() -> TestResult {
    let report = forecast(
        "let fs=require('fs'); let object=fs; const savedObject=object; const write=object.writeFileSync; fs='rebound'; object=null; write('first','x'); savedObject.writeFileSync('second','y');",
    )?;
    assert_eq!(edits(&report)?.len(), 2);
    assert_eq!(prediction(&report, 0)["target"]["path"], "first");
    assert_eq!(prediction(&report, 1)["target"]["path"], "second");
    assert_eq!(
        bindings(prediction(&report, 0))?,
        [("fs", 1), ("object", 2), ("write", 4)]
    );
    assert_eq!(
        bindings(prediction(&report, 1))?,
        [("fs", 1), ("object", 2), ("savedObject", 3)]
    );
    Ok(())
}

#[test]
fn function_snapshot_survives_rebinding_but_current_name_stops_successors() -> TestResult {
    let report = forecast(
        "const fs=require('fs'); let write=fs.writeFileSync; const saved=write; write='rebound'; saved('before','x'); write('after','y');",
    )?;
    assert_eq!(edits(&report)?.len(), 1);
    assert_eq!(prediction(&report, 0)["target"]["path"], "before");
    assert_eq!(
        bindings(prediction(&report, 0))?,
        [("fs", 1), ("saved", 3), ("write", 2)]
    );
    assert_eq!(report["javascriptBarriers"][0]["reason"], "unmodeledCall");
    assert_eq!(report["edits"][0]["laterContext"], "opaqueSuffix");
    Ok(())
}

#[test]
fn captured_alias_read_retains_its_epoch_across_path_overwrite() -> TestResult {
    let report = forecast(
        "let fs=require('fs'); const read=fs.readFileSync; const write=fs.writeFileSync; const saved=read('input','utf8'); fs=null; write('input','changed'); write('output',saved.replaceAll('old','new'));",
    )?;
    assert_eq!(edits(&report)?.len(), 2);
    let edit = prediction(&report, 1);
    assert_eq!(edit["content"]["type"], "replaceAll");
    assert_eq!(edit["content"]["input"]["filesystemEpoch"], 0);
    assert_eq!(edit["content"]["input"]["readId"], 0);
    for epoch in [0, 1] {
        assert!(dependencies(edit)?.iter().any(|dependency| {
            dependency["type"] == "filesystemEpoch" && dependency["epoch"] == epoch
        }));
    }
    assert_eq!(
        bindings(edit)?,
        [("fs", 1), ("read", 2), ("saved", 4), ("write", 3)]
    );
    Ok(())
}
