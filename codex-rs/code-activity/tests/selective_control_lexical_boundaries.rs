//! Authored lexical boundary cases; source programs are never executed.

use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::Language;
use codex_code_activity::SelectiveEditHostProfile;
use codex_code_activity::SelectiveEditInput;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::time::Duration;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const LINE_TERMINATORS: [&str; 5] = ["\n", "\r", "\r\n", "\u{2028}", "\u{2029}"];
const PROFILES: [Option<SelectiveEditHostProfile>; 2] = [
    None,
    Some(SelectiveEditHostProfile::FreshBuiltinCodeModePatchDisplayV1),
];

fn patch() -> String {
    format!(
        "await tools.apply_patch({});",
        json!("*** Begin Patch\n*** Add File: /synthetic/first.txt\n+authored\n*** End Patch\n")
    )
}

fn analyze(source: &str, profile: Option<SelectiveEditHostProfile>) -> TestResult<Value> {
    let mut analyzer = Analyzer::new()?;
    let input = SelectiveEditInput::from_source(
        "authored-control-lexical-boundaries".into(),
        Language::TypeScript,
        source,
        None,
        AnalysisLimits::new(32_768, Duration::from_secs(2)),
    )?;
    match profile {
        Some(profile) => Ok(serde_json::to_value(
            analyzer.forecast_edits_with_host_profile(input, profile),
        )?["analysis"]
            .clone()),
        None => Ok(serde_json::to_value(analyzer.forecast_edits(input))?),
    }
}

fn refused(suffix: &str) -> TestResult {
    for profile in PROFILES {
        let report = analyze(&format!("{}\n{suffix}", patch()), profile)?;
        assert_eq!(
            report["disposition"],
            json!({"unavailable": "unsupportedSource"}),
            "{profile:?}: {suffix:?}"
        );
        for field in ["edits", "sources", "barriers"] {
            assert_eq!(report[field], json!([]), "{profile:?}: {suffix:?}");
        }
    }
    Ok(())
}

fn retained(control: &str) -> TestResult {
    for profile in PROFILES {
        let report = analyze(&format!("{}\n{control}\n{}", patch(), patch()), profile)?;
        assert_eq!(report["disposition"], json!("analyzed"), "{control:?}");
        assert_eq!(report["edits"].as_array().ok_or("edits array")?.len(), 1);
        assert_eq!(report["edits"][0]["laterContext"], json!("opaqueSuffix"));
        assert_eq!(
            report["barriers"].as_array().ok_or("barriers array")?.len(),
            1
        );
        assert_eq!(report["barriers"][0]["reason"], json!("unsupportedControl"));
    }
    Ok(())
}

#[test]
fn for_initializer_cannot_replace_literal_separator_with_automatic_semicolon() -> TestResult {
    for terminator in LINE_TERMINATORS {
        for gap in [
            terminator.to_owned(),
            format!("/*;{terminator}*/"),
            format!("// comment{terminator}"),
        ] {
            refused(&format!("for(let i=0{gap} i<1;i++){{}}"))?;
        }
    }
    Ok(())
}

#[test]
fn for_condition_still_requires_its_literal_separator() -> TestResult {
    for terminator in LINE_TERMINATORS {
        for gap in [terminator.to_owned(), format!("/*{terminator}*/")] {
            refused(&format!("for(let i=0;i<1{gap} i++){{}}"))?;
        }
    }
    Ok(())
}

#[test]
fn postfix_increment_refuses_line_terminators_in_whitespace_and_comments() -> TestResult {
    for terminator in LINE_TERMINATORS {
        for gap in [
            terminator.to_owned(),
            format!("/*{terminator}*/"),
            format!("// comment{terminator}"),
        ] {
            refused(&format!("for(let i=0;i<1;i{gap}++){{}}"))?;
        }
    }
    Ok(())
}

#[test]
fn postfix_increment_accepts_same_line_whitespace_and_comments() -> TestResult {
    for gap in ["", " ", "\t", "\u{000b}", "\u{000c}", "/* comment */"] {
        retained(&format!("for(let i=0;i<1;i{gap}++){{}}"))?;
    }
    Ok(())
}

#[test]
fn prefix_increment_allows_line_terminators_in_whitespace_and_comments() -> TestResult {
    retained("for(let i=0;i<1;++i){}")?;
    for terminator in LINE_TERMINATORS {
        for gap in [
            terminator.to_owned(),
            format!("/*{terminator}*/"),
            format!("// comment{terminator}"),
        ] {
            retained(&format!("for(let i=0;i<1;++{gap}i){{}}"))?;
        }
    }
    Ok(())
}

#[test]
fn literal_for_separators_allow_multiline_headers_and_intervening_comments() -> TestResult {
    for terminator in LINE_TERMINATORS {
        for gap in [
            terminator.to_owned(),
            format!("/*{terminator}*/"),
            format!("// comment{terminator}"),
        ] {
            retained(&format!("for(let i=0{gap};{gap}i<1{gap};{gap}i++){{}}"))?;
        }
    }
    Ok(())
}

#[test]
fn ordinary_declarations_keep_automatic_semicolons_outside_for_headers() -> TestResult {
    for terminator in ["\n", "\r\n"] {
        let control = format!("{{let value=0{terminator} const next=1{terminator}}}");
        retained(&control)?;
    }
    Ok(())
}
