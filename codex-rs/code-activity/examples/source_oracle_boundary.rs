//! Capture authored source-only cases and the separate unknown-runtime boundary.
//! Source strings are inert: this program never dispatches tools or reads paths.

use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::CallId;
use codex_code_activity::CompleteScript;
use codex_code_activity::CompleteSourceDescriptionInput;
use codex_code_activity::EnvironmentAssumptions;
use codex_code_activity::GUARD_MAX_SOURCE_BYTES;
use codex_code_activity::Language;
use codex_code_activity::Request;
use serde::Deserialize;
use serde_json::json;
use std::io::BufRead;
use std::io::Write;
use std::time::Duration;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AuthoredCase {
    id: String,
    source: String,
    source_limit: Option<usize>,
    strict: bool,
    #[serde(default)]
    include_source_texts: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut analyzer = Analyzer::new()?;
    let mut output = std::io::BufWriter::new(std::io::stdout().lock());
    for line in std::io::stdin().lock().lines() {
        let case: AuthoredCase = serde_json::from_str(&line?)?;
        let limits = AnalysisLimits::new(
            case.source_limit.unwrap_or(GUARD_MAX_SOURCE_BYTES),
            Duration::from_millis(100),
        );
        let input = CompleteSourceDescriptionInput::from_source(
            CallId::from(case.id.as_str()),
            Language::TypeScript,
            &case.source,
            limits,
        );
        let mut source_texts = None;
        let mut record = match input {
            Err(error) => json!({
                "id": case.id,
                "construction": "rejected",
                "constructorError": error.to_string(),
                "description": null,
                "strictRaw": null,
            }),
            Ok(input) => {
                let description = analyzer.describe_complete(input);
                if case.include_source_texts {
                    source_texts = Some(description.source_texts().to_vec());
                }
                let strict = if case.strict {
                    let request = Request::new(
                        CallId::from(case.id.as_str()),
                        Language::TypeScript,
                        case.source,
                        None,
                    );
                    let input = CompleteScript::from_request(
                        request,
                        EnvironmentAssumptions::PersistentOrUnknown,
                        limits,
                    )?;
                    Some(analyzer.analyze_complete(input))
                } else {
                    None
                };
                json!({
                    "id": case.id,
                    "construction": "accepted",
                    "constructorError": null,
                    "description": description,
                    "strictRaw": strict,
                })
            }
        };
        if case.include_source_texts {
            record["sourceTexts"] = serde_json::to_value(source_texts)?;
        }
        serde_json::to_writer(&mut output, &record)?;
        writeln!(output)?;
    }
    Ok(())
}
