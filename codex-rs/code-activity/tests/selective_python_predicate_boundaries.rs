//! Fail-closed source and materialization boundaries; no Python is executed.

#[path = "selective_python_lazy_slices/support.rs"]
mod support;

use codex_code_activity::SnapshotPreviewError;
use codex_code_activity::SnapshotPreviewLimit;
use pretty_assertions::assert_eq;
use serde_json::Value;
use support::Fixture;
use support::analyze;
use support::transform;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn report(source: &str) -> TestResult<Value> {
    Ok(serde_json::to_value(analyze(source)?)?)
}

#[test]
fn unselected_effectful_or_unknown_rhs_is_a_barrier_without_hidden_effects() -> TestResult {
    for rhs in [
        "unknown()",
        "open('hidden','w').write('x')",
        "Path('hidden').write_text('x') == 'x'",
        "Path('hidden').read_text() == 'x'",
        "s.startswith(unknown())",
        "s == unknown",
        "'x' in [s]",
        "s == 1",
        "s",
        "True",
    ] {
        let source = format!(
            "from pathlib import Path\nopen('prefix','w')\ns='plain'\nx=(s == 'other') and ({rhs})\nopen('later','w')"
        );
        let value = report(&source)?;
        assert_eq!(value["disposition"], "analyzed", "{rhs}");
        let edits = value["edits"].as_array().ok_or("edits")?;
        assert_eq!(edits.len(), 1, "{rhs}");
        assert_eq!(
            edits[0]["details"]["prediction"]["target"]["path"],
            "prefix"
        );
        assert_eq!(
            edits[0]["details"]["prediction"]["finalState"],
            "opaqueSuccessor"
        );
        assert_eq!(
            value["pythonBarriers"].as_array().ok_or("barriers")?.len(),
            1
        );
    }
    Ok(())
}

#[test]
fn helper_spelling_and_rebound_method_names_do_not_supply_predicate_authority() -> TestResult {
    for setup in [
        "def startswith(value):\n    return value",
        "startswith=s.startswith\nstartswith='rebound'",
        "startswith=s.count",
    ] {
        let source = format!(
            "open('prefix','w')\ns='plain'\n{setup}\nx=(s == 'other') and startswith('p')\nopen('later','w')"
        );
        let value = report(&source)?;
        assert_eq!(value["edits"].as_array().ok_or("edits")?.len(), 1);
        assert_eq!(
            value["pythonBarriers"].as_array().ok_or("barriers")?.len(),
            1
        );
    }
    Ok(())
}

#[test]
fn wider_operators_chains_malformed_source_and_overflow_reject_the_whole_source() -> TestResult {
    for expression in [
        "s == s == s",
        "'a' in s == s",
        "s != 'a'",
        "'a' not in s",
        "s is s",
        "s == 'a' or s == 'b'",
        "not (s == 'a')",
        "s == 'a' and",
        "s == s[9223372036854775808:]",
    ] {
        let value = report(&format!("open('prefix','w')\ns='a'\nx={expression}"))?;
        assert_ne!(value["disposition"], "analyzed", "{expression}");
        assert!(
            value["edits"].as_array().ok_or("edits")?.is_empty(),
            "{expression}"
        );
    }
    Ok(())
}

#[test]
fn unsupported_mutation_cannot_be_hidden_by_a_later_predicate() -> TestResult {
    let value = report(
        "open('prefix','w')\ns='abc'\ns[0]='x'\nx='yes' if 'a' in s else 'no'\nopen('later','w')",
    )?;
    assert_eq!(value["edits"].as_array().ok_or("edits")?.len(), 1);
    assert_eq!(
        value["pythonBarriers"].as_array().ok_or("barriers")?.len(),
        1
    );
    Ok(())
}

#[test]
fn conjunction_recursion_and_captured_expansion_obey_construction_limits() -> TestResult {
    let deep = (0..70).fold("s == s".to_owned(), |left, _| {
        format!("({left}) and s == s")
    });
    let mut expanded = "p=s == s\n".to_owned();
    for _ in 0..20 {
        expanded.push_str("p=p and p\n");
    }
    for body in [format!("x={deep}"), expanded] {
        let value = report(&format!(
            "open('prefix','w')\ns='abc'\n{body}\nopen('later','w')"
        ))?;
        assert_ne!(value["disposition"], "analyzed");
        assert!(value["edits"].as_array().ok_or("edits")?.is_empty());
    }
    Ok(())
}

#[test]
fn comparison_and_conjunction_evaluation_share_node_depth_and_work_budgets() -> TestResult {
    let expression = "'yes' if s == 'plain' and 'a' in s else 'no'";
    assert_eq!(
        transform(expression, b"plain")?.preview()?.after_bytes,
        b"yes"
    );
    let source = format!(
        "from pathlib import Path\ns='plain'\nPath('out').write_text({expression},encoding='utf-8')"
    );
    for (limit, expected) in [
        ("nodes", SnapshotPreviewLimit::Nodes),
        ("depth", SnapshotPreviewLimit::Depth),
        ("work", SnapshotPreviewLimit::Work),
    ] {
        let mut fixture = Fixture::python(&source, None)?;
        match limit {
            "nodes" => fixture.input.limits.nodes = 1,
            "depth" => fixture.input.limits.depth = 1,
            _ => fixture.input.limits.work = 1,
        }
        assert!(matches!(
            fixture.preview(),
            Err(SnapshotPreviewError::ResourceLimit { limit }) if limit == expected
        ));
    }
    Ok(())
}
