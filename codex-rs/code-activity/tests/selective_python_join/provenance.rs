use super::support::Fixture;
use super::support::TestResult;
use super::support::case;
use super::support::span_text;
use super::support::text;
use codex_code_activity::PythonDependency;
use codex_code_activity::PythonEdit;
use codex_code_activity::PythonOperation;
use codex_code_activity::SelectiveEditDetails;
use codex_code_activity::TextRecipe;
use pretty_assertions::assert_eq;
use serde_json::json;

fn prediction(fixture: &Fixture) -> TestResult<&PythonEdit> {
    let edit = fixture.analysis.edits().first().ok_or("one forecast")?;
    let SelectiveEditDetails::Python { prediction, .. } = &edit.details else {
        return Err("Python forecast".into());
    };
    Ok(prediction)
}

fn recipe(prediction: &PythonEdit) -> TestResult<&TextRecipe> {
    let PythonOperation::WriteText { content, .. } = &prediction.operation else {
        return Err("text write forecast".into());
    };
    Ok(content)
}

fn leaves<'a>(value: &'a TextRecipe, into: &mut Vec<&'a TextRecipe>) {
    match value {
        TextRecipe::Concat { left, right, .. } => {
            leaves(left, into);
            leaves(right, into);
        }
        _ => into.push(value),
    }
}

#[test]
fn lowered_concat_keeps_token_provenance_and_exact_sink() -> TestResult {
    let row = case("J01")?;
    let source = text(&row, "source")?;
    let fixture = Fixture::new(&row)?;
    let prediction = prediction(&fixture)?;
    let TextRecipe::Concat { evidence, .. } = recipe(prediction)? else {
        return Err("lowered concatenation".into());
    };
    assert_eq!(span_text(source, evidence)?, "','.join(['red', 'blue'])");
    assert_eq!(
        span_text(source, &prediction.evidence)?,
        "Path('result.txt').write_text(','.join(['red', 'blue']), encoding='utf-8')"
    );
    let mut values = Vec::new();
    leaves(recipe(prediction)?, &mut values);
    let mut tokens = Vec::new();
    for value in values {
        let TextRecipe::Literal { evidence, .. } = value else {
            return Err("literal leaf".into());
        };
        tokens.push(span_text(source, evidence)?);
    }
    assert_eq!(tokens, vec!["'red'", "','", "'blue'"]);
    assert!(
        prediction
            .predecessor_conditions
            .iter()
            .any(|span| span_text(source, span).is_ok_and(|s| s == "','.join(['red', 'blue'])"))
    );
    assert_eq!(fixture.preview()?.after_bytes, b"red,blue");
    Ok(())
}

#[test]
fn singleton_keeps_its_original_token_origin() -> TestResult {
    let row = case("J05")?;
    let source = text(&row, "source")?;
    let fixture = Fixture::new(&row)?;
    let prediction = prediction(&fixture)?;
    let TextRecipe::Literal { evidence, value } = recipe(prediction)? else {
        return Err("constant recipe".into());
    };
    assert_eq!(span_text(source, evidence)?, "'solo'");
    assert_eq!(value, text(&row, "after")?);
    assert!(
        prediction
            .predecessor_conditions
            .iter()
            .any(|span| { span_text(source, span).is_ok_and(|s| s.contains(".join(")) })
    );
    assert_eq!(
        fixture.preview()?.after_bytes,
        text(&row, "after")?.as_bytes()
    );
    Ok(())
}

#[test]
fn captured_separator_keeps_its_old_binding_version() -> TestResult {
    let row = case("J10")?;
    let fixture = Fixture::new(&row)?;
    let prediction = prediction(&fixture)?;
    for (expected_name, expected_version) in [("s", 2), ("f", 3)] {
        assert!(prediction.dependencies.iter().any(|dependency| {
            matches!(dependency, PythonDependency::Binding { name, version, .. }
                if name == expected_name && *version == expected_version)
        }));
    }
    assert!(!prediction.dependencies.iter().any(|dependency| {
        matches!(dependency, PythonDependency::Binding { name, version, .. }
            if name == "s" && *version == 4)
    }));
    assert_eq!(fixture.preview()?.after_bytes, b"a-b");
    Ok(())
}

#[test]
fn repeated_read_leaves_share_one_source_identity_and_binding() -> TestResult {
    let row = case("J12")?;
    let source = text(&row, "source")?;
    let fixture = Fixture::new(&row)?;
    let prediction = prediction(&fixture)?;
    let mut values = Vec::new();
    leaves(recipe(prediction)?, &mut values);
    let mut reads = 0;
    for value in values {
        if let TextRecipe::Read {
            read_id,
            filesystem_epoch,
            evidence,
            ..
        } = value
        {
            assert_eq!(*read_id, 0);
            assert_eq!(*filesystem_epoch, 0);
            assert_eq!(evidence.source_id.index(), 0);
            assert_eq!(
                span_text(source, evidence)?,
                "Path('input').read_text(encoding='utf-8')"
            );
            reads += 1;
        }
    }
    assert_eq!(reads, 2);
    assert_eq!(
        prediction
            .dependencies
            .iter()
            .filter(|dependency| matches!(dependency, PythonDependency::ReadValue { .. }))
            .count(),
        1
    );
    let preview = fixture.preview()?;
    assert_eq!(preview.after_bytes, b"x/x");
    assert_eq!(preview.read_bindings.len(), 1);
    Ok(())
}

#[test]
fn singleton_result_keeps_separator_binding_dependencies() -> TestResult {
    let source = "from pathlib import Path\ns = ','\n\
        v = Path('input').read_text(encoding='utf-8')\n\
        Path('result.txt').write_text(s.join([v]), encoding='utf-8')";
    let row = json!({ "source": source, "read": "read" });
    let fixture = Fixture::new(&row)?;
    let prediction = prediction(&fixture)?;
    assert!(prediction.dependencies.iter().any(|dependency| {
        matches!(dependency, PythonDependency::Binding { name, version, .. }
            if name == "s" && *version == 2)
    }));
    let preview = fixture.preview()?;
    assert_eq!(preview.after_bytes, b"read");
    assert_eq!(preview.read_bindings.len(), 1);
    Ok(())
}

#[test]
fn discarded_join_retains_its_completion_condition_before_a_later_write() -> TestResult {
    let source = "from pathlib import Path\n','.join(['a', 'b'])\nPath('result.txt').write_text('after', encoding='utf-8')";
    let fixture = Fixture::new(&json!({ "source": source }))?;
    let prediction = prediction(&fixture)?;
    assert!(
        prediction.predecessor_conditions.iter().any(|span| {
            span_text(source, span).is_ok_and(|text| text == "','.join(['a', 'b'])")
        })
    );
    assert_eq!(fixture.preview()?.after_bytes, b"after");
    Ok(())
}
