use super::support::TestResult;
use super::support::analyze;
use super::support::forecast;
use super::support::prediction;
use super::support::text;
use pretty_assertions::assert_eq;

const PREFIX: &str = "open('prefix', 'w', encoding='utf-8').write('kept')\n";

#[test]
fn opaque_body_alias_escape_rebinding_and_second_write_discard_only_pending_bytes() -> TestResult {
    for suffix in [
        "unknown()",
        "alias = f",
        "saved = f.write",
        "holder = [f]",
        "consume(f)",
        "f = 'rebound'",
        "f += 'rebound'",
        "f.write('second')",
        "f.seek(0)",
        "f.flush()",
        "f.close(1)",
    ] {
        let source = format!(
            "{PREFIX}with open('owned', 'w', encoding='utf-8') as f:\n    f.write('pending')\n    {suffix}\nopen('later', 'w').write('unreached')"
        );
        let report = forecast(&source)?;
        assert_eq!(report["disposition"], "analyzed", "{source}");
        assert_eq!(
            report["edits"].as_array().ok_or("missing edits")?.len(),
            3,
            "{source}"
        );
        assert_eq!(
            prediction(&report, 1)["operation"]["content"]["value"],
            "kept"
        );
        assert_eq!(prediction(&report, 2)["operation"]["type"], "truncate");
        assert_eq!(prediction(&report, 2)["target"]["path"], "owned");
        assert_eq!(prediction(&report, 2)["finalState"], "opaqueSuccessor");
    }
    Ok(())
}

#[test]
fn separate_file_operations_inside_live_context_stop_before_aliasing_effects() -> TestResult {
    for effect in [
        "open('owned', 'w')",
        "open('source', encoding='utf-8').read()",
        "Path('owned').write_text('replacement')",
        "Path('source').read_text()",
    ] {
        for before in ["", "    f.write('pending')\n"] {
            let source = format!(
                "from pathlib import Path\n{PREFIX}with open('owned', 'w', encoding='utf-8') as f:\n{before}    {effect}"
            );
            let report = forecast(&source)?;
            assert_eq!(report["disposition"], "analyzed", "{source}");
            assert_eq!(
                report["edits"].as_array().ok_or("missing edits")?.len(),
                3,
                "{source}"
            );
            assert_eq!(prediction(&report, 2)["operation"]["type"], "truncate");
        }
    }
    Ok(())
}

#[test]
fn read_handle_has_one_full_read_and_never_becomes_a_new_read_at_its_cursor() -> TestResult {
    for body in [
        "text=f.read()\n    second=f.read()",
        "text=f.read(1)",
        "text=f.read()+f.read()",
        "f.close()\n    text=f.read()",
        "f.write('bad mode')",
    ] {
        let source = format!(
            "{PREFIX}with open('input', encoding='utf-8') as f:\n    {body}\nopen('later', 'w').write('unreached')"
        );
        let report = forecast(&source)?;
        assert_eq!(report["disposition"], "analyzed", "{source}");
        assert_eq!(
            report["edits"].as_array().ok_or("missing edits")?.len(),
            2,
            "{source}"
        );
        assert_eq!(
            report["pythonBarriers"]
                .as_array()
                .ok_or("missing barriers")?
                .len(),
            1
        );
    }
    Ok(())
}

#[test]
fn closed_handle_cannot_write_inside_or_after_its_context() -> TestResult {
    for source in [
        "with open('owned', 'w', encoding='utf-8') as f:\n    f.close()\n    f.write('no')",
        "with open('owned', 'w', encoding='utf-8') as f:\n    pass\nf.write('no')",
    ] {
        let report = forecast(source)?;
        assert_eq!(report["disposition"], "analyzed");
        assert_eq!(report["edits"].as_array().ok_or("missing edits")?.len(), 1);
        assert_eq!(prediction(&report, 0)["operation"]["type"], "truncate");
    }
    Ok(())
}

#[test]
fn aliased_or_rebound_open_is_outside_the_context_capability_boundary() -> TestResult {
    for (setup, manager) in [
        ("alias=open\n", "alias"),
        ("open=open\n", "open"),
        ("open='shadowed'\n", "open"),
        ("def open(path):\n    return path\n", "open"),
    ] {
        let source = format!(
            "{PREFIX}{setup}with {manager}('bad', 'w', encoding='utf-8') as f:\n    f.write('no')"
        );
        let report = forecast(&source)?;
        assert_eq!(report["disposition"], "analyzed", "{source}");
        assert_eq!(
            report["edits"].as_array().ok_or("missing edits")?.len(),
            2,
            "{source}"
        );
        assert_eq!(prediction(&report, 1)["finalState"], "opaqueSuccessor");
    }
    Ok(())
}

#[test]
fn unsupported_modes_and_encoding_options_never_emit_owned_content() -> TestResult {
    for arguments in [
        "'bad', 'r+', encoding='utf-8'",
        "'bad', 'a', encoding='utf-8'",
        "'bad', 'x', encoding='utf-8'",
        "'bad', 'wb'",
        "'bad', 'w', encoding='latin-1'",
        "'bad', 'w', encoding=enc",
        "'bad', 'w', encoding='utf-8', newline='invalid'",
    ] {
        let source =
            format!("{PREFIX}enc='utf-8'\nwith open({arguments}) as f:\n    f.write('no')");
        let report = forecast(&source)?;
        assert_eq!(report["disposition"], "analyzed", "{source}");
        assert_eq!(
            report["edits"].as_array().ok_or("missing edits")?.len(),
            2,
            "{source}"
        );
    }
    let report = forecast("with open('bad', 'w') as f:\n    f.write('no')")?;
    assert_eq!(report["edits"].as_array().ok_or("missing edits")?.len(), 1);
    assert_eq!(prediction(&report, 0)["operation"]["type"], "truncate");
    Ok(())
}

#[test]
fn unsupported_nested_execution_keeps_truncation_without_running_the_body() -> TestResult {
    for control in [
        "for text in ['hidden']:\n        f.write(text)",
        "with open('hidden', 'w', encoding='utf-8') as other:\n        other.write('no')",
        "def deferred():\n        return 'unused'",
    ] {
        let source =
            format!("{PREFIX}with open('owned', 'w', encoding='utf-8') as f:\n    {control}");
        let report = forecast(&source)?;
        assert_eq!(report["disposition"], "analyzed", "{source}");
        assert_eq!(
            report["edits"].as_array().ok_or("missing edits")?.len(),
            3,
            "{source}"
        );
        assert_eq!(prediction(&report, 2)["operation"]["type"], "truncate");
    }
    Ok(())
}

#[test]
fn malformed_context_syntax_rejects_the_whole_source_before_any_forecast() -> TestResult {
    for suffix in [
        "with open('bad', 'w', encoding='utf-8') as f, :\n    f.write('no')",
        "async with open('bad', 'w', encoding='utf-8') as f:\n    f.write('no')",
        "with open('bad', 'w', encoding='utf-8') as f:\nf.write('no')",
        "with open('bad', 'w', encoding='utf-8') as f:\n    f.write('no'",
    ] {
        let analysis = analyze(&format!("{PREFIX}{suffix}"))?;
        assert!(analysis.edits().is_empty(), "{suffix}");
    }
    Ok(())
}

#[test]
fn source_only_conditional_prefix_survives_a_later_context_barrier() -> TestResult {
    let source = "from pathlib import Path\ns='abc'\nPath('prefix').write_text(s if s.startswith('a') else 'other', encoding='utf-8')\nwith open('owned', 'w', encoding='utf-8') as f:\n    f.write('pending')\n    opaque()";
    let report = forecast(source)?;
    assert_eq!(report["disposition"], "analyzed");
    assert_eq!(report["edits"].as_array().ok_or("missing edits")?.len(), 2);
    assert_eq!(
        prediction(&report, 0)["operation"]["content"]["type"],
        "conditional"
    );
    assert_eq!(prediction(&report, 0)["target"]["path"], "prefix");
    assert_eq!(prediction(&report, 0)["finalState"], "opaqueSuccessor");
    assert_eq!(
        text(source, &report["pythonBarriers"][0]["evidence"])?,
        "opaque"
    );
    Ok(())
}

#[test]
fn multiple_managers_nested_wrappers_and_tuple_managers_never_create_owned_handles() -> TestResult {
    for context in [
        "open('first', 'w', encoding='utf-8') as f, open('second', 'w', encoding='utf-8') as g",
        "(open('first', 'w', encoding='utf-8') as f, open('second', 'w', encoding='utf-8') as g,)",
        "((open('out', 'w', encoding='utf-8') as f))",
        "((open('out', 'w', encoding='utf-8') as f,))",
        "((open('out', 'w', encoding='utf-8') as f),)",
        "((open('out', 'w', encoding='utf-8') as f,),)",
        "(open('out', 'w', encoding='utf-8'),) as f",
        "((open('out', 'w', encoding='utf-8'),) as f,)",
        "() as f",
    ] {
        let source = format!("with {context}:\n    f.write('no')");
        let analysis = analyze(&source)?;
        assert!(analysis.edits().is_empty(), "{source}");
    }
    Ok(())
}
