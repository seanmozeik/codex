//! The Node source context must never be inferred from the TS grammar alone.

use codex_code_activity::ActivityError;
use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::Language;
use codex_code_activity::SelectiveCarrierBasis;
use codex_code_activity::SelectiveEditAnalysis;
use codex_code_activity::SelectiveEditDetails;
use codex_code_activity::SelectiveEditInput;

const SCRIPT: &str = "const fs = require('node:fs'); fs.writeFileSync('note.txt', 'ok', 'utf8');";

fn shell(source: &str) -> Result<SelectiveEditAnalysis, ActivityError> {
    let mut analyzer = Analyzer::new()?;
    let input = SelectiveEditInput::from_source(
        "carrier".into(),
        Language::Shell,
        source,
        None,
        AnalysisLimits::default(),
    )?;
    Ok(analyzer.forecast_edits(input))
}

#[test]
fn quoted_node_heredoc_emits_a_node_cli_assumption() -> Result<(), ActivityError> {
    let analysis = shell(&format!("node <<'JS'\n{SCRIPT}\nJS\n"))?;
    let [edit] = analysis.edits() else {
        panic!("expected one conditional Node edit: {analysis:?}");
    };
    assert!(matches!(
        edit.details,
        SelectiveEditDetails::JavaScript { .. }
    ));
    assert!(
        edit.carrier_assumptions.iter().any(|assumption| {
            assumption.basis == SelectiveCarrierBasis::StandardNodeCommonJsCli
        })
    );
    Ok(())
}

#[test]
fn unsupported_runtimes_and_module_modes_do_not_enter_commonjs() -> Result<(), ActivityError> {
    for executable in [
        "bun",
        "deno",
        "tsx",
        "ts-node",
        "node --input-type=module",
        "node --require startup.js",
        "env node",
    ] {
        let analysis = shell(&format!("{executable} <<'JS'\n{SCRIPT}\nJS\n"))?;
        assert!(analysis.edits().is_empty(), "{executable}: {analysis:?}");
    }
    Ok(())
}

#[test]
fn a_typescript_grammar_request_does_not_declare_node_semantics() {
    let mut analyzer = Analyzer::new().expect("test parser");
    let input = SelectiveEditInput::from_source(
        "code-mode".into(),
        Language::TypeScript,
        SCRIPT,
        None,
        AnalysisLimits::default(),
    )
    .expect("bounded test source");
    assert!(analyzer.forecast_edits(input).edits().is_empty());
}

#[test]
fn an_explicit_node_context_retains_its_standard_api_hypotheses() {
    let mut analyzer = Analyzer::new().expect("test parser");
    let input = SelectiveEditInput::from_node_commonjs(
        "node-source".into(),
        SCRIPT,
        None,
        AnalysisLimits::default(),
    )
    .expect("bounded test source");
    let analysis = analyzer.forecast_edits(input);
    let [edit] = analysis.edits() else {
        panic!("expected one conditional Node edit: {analysis:?}");
    };
    let SelectiveEditDetails::JavaScript { assumptions, .. } = &edit.details else {
        panic!("wrong source semantics");
    };
    assert!(!assumptions.is_empty());
}

#[test]
fn an_unknown_shell_predecessor_prevents_a_later_node_forecast() -> Result<(), ActivityError> {
    let analysis = shell(&format!("unknown_command; node <<'JS'\n{SCRIPT}\nJS\n"))?;
    assert!(analysis.edits().is_empty());
    Ok(())
}
