//! Inert JSONL boundary: decode, forecast, bind supplied bytes, materialize,
//! serialize. No input source is executed and no target path is opened.
//! `--benchmark` repeats that complete boundary; BENCH_ROUNDS is in 1..=100.

use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::CallId;
use codex_code_activity::ConditionalSnapshotPreview;
use codex_code_activity::GUARD_MAX_SOURCE_BYTES;
use codex_code_activity::Language;
use codex_code_activity::PythonWriteEndpoint;
use codex_code_activity::SelectiveEditAnalysis;
use codex_code_activity::SelectiveEditInput;
use codex_code_activity::SnapshotPreviewError;
use codex_code_activity::SnapshotPreviewLimit;
use codex_code_activity::SnapshotTextProfile;
#[path = "snapshot_preview_boundary/bindings.rs"]
mod bindings;

#[cfg(test)]
#[path = "snapshot_preview_boundary/endpoint_tests.rs"]
mod endpoint_tests;

use bindings::ReadInput;
use bindings::TargetInput;
use bindings::bind;
use serde::Deserialize;
use serde::Serialize;
use serde_json::json;
use std::io::BufRead;
use std::io::Read;
use std::io::Write;
use std::time::Duration;
use std::time::Instant;

const MAX_LINE_BYTES: usize = 8_388_608;
const MAX_OUTPUT_BYTES: usize = 33_554_432;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Case {
    id: String,
    language: InputLanguage,
    source: String,
    cwd: Option<String>,
    snapshot_call_id: CallId,
    #[serde(default)]
    text_profile: SnapshotTextProfile,
    #[serde(default)]
    reads: Vec<ReadInput>,
    #[serde(default)]
    targets: Vec<TargetInput>,
    #[serde(default)]
    python_write_endpoint: Option<PythonWriteEndpoint>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
enum InputLanguage {
    Python,
    Shell,
    NodeCommonJs,
}

#[derive(Serialize)]
#[serde(tag = "disposition", rename_all = "camelCase")]
enum Outcome {
    Preview {
        ordinal: usize,
        preview: Box<ConditionalSnapshotPreview>,
    },
    Refused {
        ordinal: usize,
        reason: SnapshotPreviewError,
    },
    InvalidBinding {
        ordinal: usize,
        reason: &'static str,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Record<'a> {
    schema: &'static str,
    id: &'a str,
    construction: &'static str,
    analysis: &'a SelectiveEditAnalysis,
    source_texts: &'a [String],
    previews: Vec<Outcome>,
}

fn main() -> std::io::Result<()> {
    let rounds = rounds()?;
    let mut stdin = std::io::stdin().lock();
    let mut stdout = std::io::BufWriter::new(std::io::stdout().lock());
    for _ in 0..4096 {
        let mut line = String::new();
        let count = stdin
            .by_ref()
            .take((MAX_LINE_BYTES + 1) as u64)
            .read_line(&mut line)?;
        if count == 0 {
            return Ok(());
        }
        if count > MAX_LINE_BYTES {
            return Err(invalid("input line byte limit exceeded"));
        }
        let mut times = Vec::with_capacity(rounds);
        let mut baseline = Vec::new();
        for round in 0..rounds {
            let start = Instant::now();
            let bytes = capture(&line)?;
            let elapsed = start.elapsed().as_nanos();
            if round == 0 {
                baseline = bytes;
            } else if bytes != baseline {
                return Err(invalid("preview boundary changed between repetitions"));
            }
            times.push(elapsed);
        }
        stdout.write_all(b"{\"elapsedNs\":")?;
        serde_json::to_writer(&mut stdout, &times).map_err(std::io::Error::other)?;
        stdout.write_all(b",\"result\":")?;
        stdout.write_all(&baseline)?;
        writeln!(stdout, "}}")?;
    }
    Err(invalid("input row limit exceeded"))
}

fn capture(line: &str) -> std::io::Result<Vec<u8>> {
    let case: Case = match serde_json::from_str(line) {
        Ok(case) => case,
        Err(error) => {
            return encode(
                &json!({"schema":"conditionalSnapshotPreviewCapture/v1", "construction":"rejected", "reason":error.to_string()}),
            );
        }
    };
    if case.targets.len() > 16 || case.reads.len() > 256 {
        return encode(
            &json!({"schema":"conditionalSnapshotPreviewCapture/v1", "id":case.id, "construction":"rejected", "reason":"snapshot record limit exceeded"}),
        );
    }
    let limits = AnalysisLimits::new(GUARD_MAX_SOURCE_BYTES, Duration::from_secs(2));
    let input = match case.language {
        InputLanguage::NodeCommonJs => SelectiveEditInput::from_node_commonjs(
            case.id.as_str().into(),
            &case.source,
            case.cwd.as_deref(),
            limits,
        ),
        InputLanguage::Python | InputLanguage::Shell => SelectiveEditInput::from_source(
            case.id.as_str().into(),
            match case.language {
                InputLanguage::Python => Language::Python,
                _ => Language::Shell,
            },
            &case.source,
            case.cwd.as_deref(),
            limits,
        ),
    };
    let input = match input {
        Ok(input) => input.with_python_write_endpoint(
            case.python_write_endpoint
                .unwrap_or_else(|| bindings::requested_endpoint(&case.targets)),
        ),
        Err(error) => {
            return encode(
                &json!({"schema":"conditionalSnapshotPreviewCapture/v1", "id":case.id, "construction":"rejected", "reason":format!("{error:?}")}),
            );
        }
    };
    let analysis = Analyzer::new()
        .map_err(std::io::Error::other)?
        .forecast_edits(input);
    let mut retained_bytes = 0usize;
    let mut previews = Vec::with_capacity(case.targets.len());
    for target in &case.targets {
        let outcome = match bind(&case, &analysis, target) {
            Ok(input) => match input.materialize(&analysis, target.ordinal) {
                Ok(preview) => {
                    retained_bytes = retained_bytes
                        .saturating_add(preview.after_bytes.len())
                        .saturating_add(preview.before_bytes.as_ref().map_or(0, Vec::len))
                        .saturating_add(preview.unified_diff.len())
                        .saturating_add(
                            preview
                                .logical_stream
                                .as_ref()
                                .map_or(0, |logical| logical.payload_bytes.len()),
                        );
                    if retained_bytes > MAX_LINE_BYTES {
                        Outcome::Refused {
                            ordinal: target.ordinal,
                            reason: SnapshotPreviewError::ResourceLimit {
                                limit: SnapshotPreviewLimit::Output,
                            },
                        }
                    } else {
                        Outcome::Preview {
                            ordinal: target.ordinal,
                            preview: Box::new(preview),
                        }
                    }
                }
                Err(reason) => Outcome::Refused {
                    ordinal: target.ordinal,
                    reason,
                },
            },
            Err(reason) => Outcome::InvalidBinding {
                ordinal: target.ordinal,
                reason,
            },
        };
        previews.push(outcome);
    }
    encode(&Record {
        schema: "conditionalSnapshotPreviewCapture/v1",
        id: &case.id,
        construction: "accepted",
        analysis: &analysis,
        source_texts: analysis.source_texts(),
        previews,
    })
}

fn rounds() -> std::io::Result<usize> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() {
        return Ok(1);
    }
    if args != ["--benchmark"] {
        return Err(invalid("expected --benchmark or no arguments"));
    }
    let count = match std::env::var("BENCH_ROUNDS") {
        Ok(value) => value.parse::<usize>().map_err(std::io::Error::other)?,
        Err(std::env::VarError::NotPresent) => 9,
        Err(error) => return Err(std::io::Error::other(error)),
    };
    if !(1..=100).contains(&count) {
        return Err(invalid("BENCH_ROUNDS must be in 1..=100"));
    }
    Ok(count)
}

fn invalid(message: &'static str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, message)
}

fn encode(value: &impl Serialize) -> std::io::Result<Vec<u8>> {
    let mut output = BoundedOutput(Vec::new());
    serde_json::to_writer(&mut output, value).map_err(std::io::Error::other)?;
    Ok(output.0)
}

struct BoundedOutput(Vec<u8>);

impl Write for BoundedOutput {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.0.len().saturating_add(bytes.len()) > MAX_OUTPUT_BYTES {
            return Err(invalid("serialized capture byte limit exceeded"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
