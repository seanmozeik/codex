use super::support::TestResult;
use super::support::bindings;
use super::support::dependencies;
use super::support::edits;
use super::support::forecast;
use super::support::prediction;
use super::support::span_text;
use pretty_assertions::assert_eq;

#[test]
fn descriptor_methods_keep_ownership_cursor_and_close_conditions_through_aliases() -> TestResult {
    let source = "let fs=require('fs'); const open=fs.openSync; const write=fs.writeSync; const close=fs.closeSync; const finish=fs.writeFileSync; const fd=open('out','w'); fs=null; write(fd,'é'); write(fd,'Z',4,'utf8'); write(fd,'!'); close(fd); finish('marker','done');";
    let report = forecast(source)?;
    assert_eq!(edits(&report)?.len(), 5);
    let truncate = prediction(&report, 0);
    assert_eq!(truncate["writeEffect"]["type"], "truncate");
    assert_eq!(truncate["writeEffect"]["descriptorId"], 0);
    assert_eq!(span_text(source, &truncate["evidence"])?, "open('out','w')");
    assert_eq!(bindings(truncate)?, [("fs", 1), ("open", 2)]);
    for (ordinal, offset, position) in [(1, 0, "cursor"), (2, 4, "absolute"), (3, 2, "cursor")] {
        let edit = prediction(&report, ordinal);
        assert_eq!(edit["target"]["path"], "out");
        assert_eq!(edit["writeEffect"]["type"], "descriptorWrite");
        assert_eq!(edit["writeEffect"]["descriptorId"], 0);
        assert_eq!(edit["writeEffect"]["openedAt"], truncate["evidence"]);
        assert_eq!(edit["writeEffect"]["byteOffset"], offset);
        assert_eq!(edit["writeEffect"]["position"], position);
        assert_eq!(
            bindings(edit)?,
            [("fd", 6), ("fs", 1), ("open", 2), ("write", 3)]
        );
        for assumption in ["regularDescriptorFiles", "completeDescriptorWrites"] {
            assert!(dependencies(edit)?.iter().any(|dependency| {
                dependency["type"] == "capability" && dependency["assumption"] == assumption
            }));
        }
    }
    let conditions = prediction(&report, 4)["predecessorConditions"]
        .as_array()
        .ok_or("missing predecessor conditions")?;
    let condition_text = conditions
        .iter()
        .map(|span| span_text(source, span))
        .collect::<TestResult<Vec<_>>>()?;
    assert_eq!(condition_text.last(), Some(&"close(fd)"));
    Ok(())
}

#[test]
fn aliased_descriptor_calls_keep_direct_const_owner_and_flag_requirements() -> TestResult {
    let prefix = "const fs=require('fs'); let open=fs.openSync; const write=fs.writeSync;";
    for body in [
        "let fd=open('out','w'); write(fd,'x');",
        "open('out','w');",
        "write(open('out','w'),'x');",
        "const fd=open('out','a+'); write(fd,'x');",
        "const fd=open('out','r'); write(fd,'x');",
        "const fd=open('out','wx'); write(fd,'x');",
        "const fd=open('out','w',420); write(fd,'x');",
    ] {
        let report = forecast(&format!("{prefix} {body}"))?;
        assert!(edits(&report)?.is_empty(), "{body}");
        assert_eq!(
            report["javascriptBarriers"].as_array().map(Vec::len),
            Some(1)
        );
    }
    Ok(())
}

#[test]
fn filesystem_function_aliases_do_not_permit_mutable_descriptor_aliases_or_escapes() -> TestResult {
    let prefix = "const fs=require('fs'); const open=fs.openSync; const write=fs.writeSync; const close=fs.closeSync; const fd=open('out','r+');";
    for body in [
        "let alias=fd; write(alias,'x');",
        "fd=0; write(fd,'x');",
        "unknown(fd); write(fd,'x');",
        "close(fd); write(fd,'x');",
        "close(fd); close(fd);",
        "write(1,'x');",
        "const pathWrite=fs.writeFileSync; pathWrite(fd,'x');",
        "const count=write(fd,'x');",
    ] {
        let report = forecast(&format!("{prefix} {body} fs.writeFileSync('after','y');"))?;
        assert!(edits(&report)?.is_empty(), "{body}");
        assert_eq!(
            report["javascriptBarriers"].as_array().map(Vec::len),
            Some(1)
        );
    }
    Ok(())
}

#[test]
fn aliased_reopen_uses_a_new_descriptor_identity_and_keeps_the_old_one_closed() -> TestResult {
    let report = forecast(
        "const fs=require('fs'); const open=fs.openSync; const write=fs.writeSync; const close=fs.closeSync; const first=open('a','r+'); close(first); const second=open('b','r+'); write(second,'X'); write(first,'wrong');",
    )?;
    assert_eq!(edits(&report)?.len(), 1);
    assert_eq!(prediction(&report, 0)["target"]["path"], "b");
    assert_eq!(prediction(&report, 0)["writeEffect"]["descriptorId"], 1);
    assert_eq!(
        report["javascriptBarriers"][0]["reason"],
        "unsupportedOperation"
    );
    Ok(())
}

#[test]
fn write_plus_open_truncates_before_cursor_writes_through_canonical_aliases() -> TestResult {
    for opening in ["fs.openSync", "open"] {
        let source = format!(
            "const fs=require('node:fs'); const open=fs.openSync; const fd={opening}('out','w+'); fs.writeSync(fd,'é'); fs.writeSync(fd,'!'); fs.closeSync(fd);"
        );
        let report = forecast(&source)?;
        assert_eq!(edits(&report)?.len(), 3);
        assert!(report.get("javascriptBarriers").is_none());
        let truncate = prediction(&report, 0);
        assert_eq!(truncate["writeEffect"]["type"], "truncate");
        assert_eq!(truncate["content"]["value"], "");
        for (ordinal, offset) in [(1, 0), (2, 2)] {
            let write = prediction(&report, ordinal);
            assert_eq!(write["writeEffect"]["byteOffset"], offset);
            assert_eq!(write["writeEffect"]["position"], "cursor");
            assert_eq!(write["writeEffect"]["openedAt"], truncate["evidence"]);
            assert!(
                write["predecessorConditions"]
                    .as_array()
                    .ok_or("missing conditions")?
                    .contains(&truncate["evidence"])
            );
            assert!(dependencies(write)?.iter().any(|dependency| {
                dependency["type"] == "filesystemEpoch" && dependency["epoch"] == ordinal
            }));
            for assumption in ["regularDescriptorFiles", "completeDescriptorWrites"] {
                assert!(dependencies(write)?.iter().any(|dependency| {
                    dependency["type"] == "capability" && dependency["assumption"] == assumption
                }));
            }
        }
    }
    Ok(())
}

#[test]
fn write_plus_flag_snapshot_survives_later_rebinding() -> TestResult {
    let report = forecast(
        "const fs=require('fs'); let flags='w+'; const fd=fs.openSync('out',flags); flags='a'; fs.writeSync(fd,'X',0,'utf8');",
    )?;
    assert_eq!(edits(&report)?.len(), 2);
    assert_eq!(prediction(&report, 0)["writeEffect"]["type"], "truncate");
    assert!(bindings(prediction(&report, 1))?.contains(&("flags", 2)));
    assert!(!bindings(prediction(&report, 1))?.contains(&("flags", 4)));
    Ok(())
}

#[test]
fn write_plus_retains_unsupported_flags_mutable_alias_and_predecessor_barriers() -> TestResult {
    for flags in ["a+", "wx", "wx+", "w+x", "as+", "r"] {
        let report = forecast(&format!(
            "const fs=require('fs'); const fd=fs.openSync('out','{flags}'); fs.writeSync(fd,'X');"
        ))?;
        assert!(edits(&report)?.is_empty(), "{flags}");
    }
    for suffix in [
        "let alias=fd; fs.writeSync(alias,'X');",
        "unknown(); fs.writeSync(fd,'X');",
        "fs.closeSync(fd); fs.writeSync(fd,'X');",
    ] {
        let report = forecast(&format!(
            "const fs=require('fs'); const fd=fs.openSync('out','w+'); {suffix}"
        ))?;
        assert_eq!(edits(&report)?.len(), 1, "{suffix}");
        assert_eq!(prediction(&report, 0)["writeEffect"]["type"], "truncate");
        assert_eq!(report["edits"][0]["laterContext"], "opaqueSuffix");
    }
    Ok(())
}
