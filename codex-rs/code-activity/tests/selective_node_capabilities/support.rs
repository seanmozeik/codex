use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::SelectiveEditInput;
use serde_json::Value;
use std::time::Duration;

pub(super) type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub(super) fn forecast(source: &str) -> TestResult<Value> {
    let input = SelectiveEditInput::from_node_commonjs(
        "authored-node-capabilities".into(),
        source,
        Some("/authored"),
        AnalysisLimits::new(32_768, Duration::from_secs(2)),
    )?;
    Ok(serde_json::to_value(
        Analyzer::new()?.forecast_edits(input),
    )?)
}

pub(super) fn edits(report: &Value) -> TestResult<&[Value]> {
    Ok(report["edits"].as_array().ok_or("missing edits")?)
}

pub(super) fn prediction(report: &Value, ordinal: usize) -> &Value {
    &report["edits"][ordinal]["details"]["prediction"]
}

pub(super) fn dependencies(edit: &Value) -> TestResult<&[Value]> {
    Ok(edit["dependencies"]
        .as_array()
        .ok_or("missing dependencies")?)
}

pub(super) fn bindings(edit: &Value) -> TestResult<Vec<(&str, u64)>> {
    let mut bindings = Vec::new();
    for dependency in dependencies(edit)? {
        if dependency["type"] == "binding" {
            bindings.push((
                dependency["name"].as_str().ok_or("missing binding name")?,
                dependency["version"]
                    .as_u64()
                    .ok_or("missing binding version")?,
            ));
        }
    }
    bindings.sort_unstable();
    Ok(bindings)
}

pub(super) fn span_text<'source>(source: &'source str, span: &Value) -> TestResult<&'source str> {
    let start = usize::try_from(span["startByte"].as_u64().ok_or("missing startByte")?)?;
    let end = usize::try_from(span["endByte"].as_u64().ok_or("missing endByte")?)?;
    Ok(source.get(start..end).ok_or("invalid evidence span")?)
}
