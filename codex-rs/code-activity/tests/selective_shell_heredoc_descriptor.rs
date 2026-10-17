//! The preview parser preserves an explicit stdin carrier without executing it.

use codex_code_activity::ActivityError;
use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::Language;
use codex_code_activity::SelectiveCarrierBasis;
use codex_code_activity::SelectiveEditDisposition;
use codex_code_activity::SelectiveEditInput;
use pretty_assertions::assert_eq;

#[test]
fn explicit_stdin_heredocs_reach_python_and_node_forecasts() -> Result<(), ActivityError> {
    for (command, body, basis) in [
        (
            "python",
            "from pathlib import Path\nPath('out').write_text('text', encoding='utf-8')\n",
            SelectiveCarrierBasis::StandardPython3Cli,
        ),
        (
            "node",
            "const fs = require('node:fs'); fs.writeFileSync('out', 'text', 'utf8');\n",
            SelectiveCarrierBasis::StandardNodeCommonJsCli,
        ),
    ] {
        let header = format!("{command} 0<<'CODE'\n");
        let source = format!("{header}{body}CODE\n");
        let input = SelectiveEditInput::from_source(
            "authored-preview-heredoc-descriptor".into(),
            Language::Shell,
            &source,
            None,
            AnalysisLimits::default(),
        )?;
        let analysis = Analyzer::new()?.forecast_edits(input);
        assert_eq!(analysis.disposition(), SelectiveEditDisposition::Analyzed);
        assert_eq!(analysis.source_texts(), &[source.clone(), body.to_owned()]);
        let [edit] = analysis.edits() else {
            panic!("expected one conditional edit: {analysis:?}");
        };
        assert_eq!(edit.evidence.source_id.index(), 1);
        assert!(
            edit.carrier_assumptions
                .iter()
                .any(|assumption| assumption.basis == basis)
        );
        let parent = analysis.sources()[1]
            .parent
            .as_ref()
            .ok_or(ActivityError::SourceLimit)?;
        assert_eq!(parent.start_byte, header.len());
        assert_eq!(parent.end_byte, header.len() + body.len());
        assert_eq!(source.get(parent.start_byte..parent.end_byte), Some(body));
    }
    Ok(())
}

const PYTHON_BODY: &str =
    "from pathlib import Path\nPath('out').write_text('text', encoding='utf-8')\n";
const NODE_BODY: &str = "const fs = require('node:fs'); fs.writeFileSync('out', 'text', 'utf8');\n";

fn forecast(source: &str) -> Result<codex_code_activity::SelectiveEditAnalysis, ActivityError> {
    let input = SelectiveEditInput::from_source(
        "authored-selective-heredoc-admission".into(),
        Language::Shell,
        source,
        None,
        AnalysisLimits::default(),
    )?;
    Ok(Analyzer::new()?.forecast_edits(input))
}

fn assert_no_child_forecast(source: &str) -> Result<(), ActivityError> {
    let analysis = forecast(source)?;
    assert!(analysis.edits().is_empty(), "{source:?}: {analysis:?}");
    assert!(
        analysis.source_texts().len() <= 1,
        "{source:?}: {analysis:?}"
    );
    assert!(
        analysis
            .sources()
            .iter()
            .all(|owned| owned.parent.is_none()),
        "{source:?}: {analysis:?}"
    );
    Ok(())
}

#[test]
fn implicit_stdin_and_explicit_spacing_controls_keep_owned_body_spans() -> Result<(), ActivityError>
{
    for (command, body) in [("python", PYTHON_BODY), ("node", NODE_BODY)] {
        for redirect in ["<<'CODE'", "0<<\"CODE\"", "0<< 'CODE'", "- 0<<'CODE'"] {
            let header = format!("{command} {redirect}\n");
            let source = format!("{header}{body}CODE\n");
            let analysis = forecast(&source)?;
            assert_eq!(analysis.disposition(), SelectiveEditDisposition::Analyzed);
            assert_eq!(analysis.source_texts(), &[source.clone(), body.to_owned()]);
            assert_eq!(analysis.edits().len(), 1, "{source:?}");
            let parent = analysis.sources()[1]
                .parent
                .as_ref()
                .ok_or(ActivityError::SourceLimit)?;
            assert_eq!(parent.start_byte, header.len());
            assert_eq!(parent.end_byte, header.len() + body.len());
            assert_eq!(source.get(parent.start_byte..parent.end_byte), Some(body));
        }
    }
    Ok(())
}

#[test]
fn explicit_stdin_empty_and_whitespace_bodies_keep_exact_child_ranges() -> Result<(), ActivityError>
{
    for command in ["python", "node"] {
        for body in ["", " \t\n\n"] {
            let header = format!("{command} 0<<'CODE'\n");
            let source = format!("{header}{body}CODE\n");
            let analysis = forecast(&source)?;
            assert_eq!(analysis.disposition(), SelectiveEditDisposition::Analyzed);
            assert_eq!(analysis.source_texts(), &[source.clone(), body.to_owned()]);
            assert!(analysis.edits().is_empty());
            let parent = analysis.sources()[1]
                .parent
                .as_ref()
                .ok_or(ActivityError::SourceLimit)?;
            assert_eq!(parent.start_byte, header.len());
            assert_eq!(parent.end_byte, header.len() + body.len());
            assert_eq!(source.get(parent.start_byte..parent.end_byte), Some(body));
        }
    }
    Ok(())
}

#[test]
fn unsupported_descriptor_spellings_never_supply_interpreter_stdin() -> Result<(), ActivityError> {
    for (command, body) in [("python", PYTHON_BODY), ("node", NODE_BODY)] {
        for descriptor in ["00", "01", "1", "2", "10", "999999999999999999999999"] {
            let source = format!("{command} {descriptor}<<'CODE'\n{body}CODE\n");
            assert_no_child_forecast(&source)?;
        }
    }
    Ok(())
}

#[test]
fn quoted_separated_and_line_continued_zero_cannot_become_a_descriptor() -> Result<(), ActivityError>
{
    for (command, body) in [("python", PYTHON_BODY), ("node", NODE_BODY)] {
        for redirect in [
            "0 <<'CODE'",
            "0\t<<'CODE'",
            "'0'<<'CODE'",
            "\"0\"<<'CODE'",
            "\\0<<'CODE'",
            "0\\\n<<'CODE'",
            "\\\n0<<'CODE'",
        ] {
            let source = format!("{command} {redirect}\n{body}CODE\n");
            assert_no_child_forecast(&source)?;
        }
    }
    Ok(())
}

#[test]
fn competing_redirections_and_header_suffixes_do_not_choose_a_stdin_body()
-> Result<(), ActivityError> {
    for (command, body) in [("python", PYTHON_BODY), ("node", NODE_BODY)] {
        for header in [
            "0<other 0<<'CODE'",
            "0<<'CODE' 0<other",
            "0<<'CODE' <other",
            "0<<'CODE' 0<&1",
            "0<&1 0<<'CODE'",
            "0<<'CODE' 1>other",
            "0<<'CODE' | cat",
            "0<<'CODE' && true",
            "0<<'CODE' argument",
        ] {
            let source = format!("{command} {header}\n{body}CODE\n");
            assert_no_child_forecast(&source)?;
        }
        let source = format!("{command} 0<<'FIRST' 0<<'SECOND'\n{body}FIRST\n{body}SECOND\n");
        assert_no_child_forecast(&source)?;
        let source = format!("0<<'CODE' {command}\n{body}CODE\n");
        assert_no_child_forecast(&source)?;
    }
    Ok(())
}

#[test]
fn descriptor_admission_preserves_literal_delimiter_and_physical_line_restrictions()
-> Result<(), ActivityError> {
    for (command, body) in [("python", PYTHON_BODY), ("node", NODE_BODY)] {
        for source in [
            format!("{command} 0<<CODE\n{body}CODE\n"),
            format!("{command} 0<<-'CODE'\n{body}CODE\n"),
            format!("{command} 0<<'CO-DE'\n{body}CO-DE\n"),
            format!("{command} 0<<'CO'\"DE\"\n{body}CODE\n"),
            format!("{command} 0<<'CODE'\r\n{body}CODE\n"),
            format!("{command} 0<<'CODE'\n{body} CODE\n"),
            format!("{command} 0<<'CODE'\n{body}CODE extra\n"),
            format!("{command} 0<<'CODE'\n{body}"),
        ] {
            assert_no_child_forecast(&source)?;
        }
    }
    Ok(())
}
