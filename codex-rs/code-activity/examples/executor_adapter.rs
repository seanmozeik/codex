//! Local JSONL host-assertion bridge. Never launches a child or verifies a runtime.
//! The controller owns readiness verification and releases only the exact echoed
//! tuple to its waiting child. Presentation fallback does not deny execution.

use codex_code_activity::AnalysisLimits;
use codex_code_activity::ExecutorContext;
use codex_code_activity::ExecutorRuntimeProfile;
use codex_code_activity::ExecutorSource;
use codex_code_activity::FinalExecutorInput;
use codex_code_activity::Language;
use codex_code_activity::OrchestratorBindings;
use serde::Deserialize;
use serde::Serialize;
use std::io;
use std::io::BufRead;
use std::io::Read;
use std::io::Write;
use std::time::Duration;

const MAX_LINE_BYTES: u64 = 256 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Launch {
    call_id: String,
    source: String,
    language: Language,
    cwd: Option<String>,
    launch_id: String,
    readiness_identity: String,
    #[serde(default)]
    profile: Profile,
    #[serde(default)]
    bindings: Bindings,
    #[serde(default)]
    kind: Kind,
    wait_ms: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Profile {
    #[default]
    Unknown,
    OrdinaryFresh,
    PreloadedPathlib,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Bindings {
    #[default]
    Unknown,
    TrustedFreshHostBindings,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Kind {
    #[default]
    Script,
    Orchestration,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Release<'a> {
    source: &'a str,
    cwd: Option<&'a str>,
    launch_id: &'a str,
    readiness_identity: &'a str,
}

#[derive(Debug, thiserror::Error)]
enum ExampleError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("input line exceeds the local protocol limit")]
    InputLimit,
}

fn main() -> Result<(), ExampleError> {
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let stdout = io::stdout();
    let mut output = stdout.lock();
    loop {
        let mut line = Vec::new();
        let read = (&mut input)
            .take(MAX_LINE_BYTES + 1)
            .read_until(b'\n', &mut line)?;
        if read == 0 {
            break;
        }
        if line.len() as u64 > MAX_LINE_BYTES {
            return Err(ExampleError::InputLimit);
        }
        let launch: Launch = serde_json::from_slice(&line)?;
        let profile = match launch.profile {
            Profile::Unknown => ExecutorRuntimeProfile::Unknown,
            Profile::OrdinaryFresh => ExecutorRuntimeProfile::OrdinaryFresh,
            Profile::PreloadedPathlib => ExecutorRuntimeProfile::PreloadedPathlib,
        };
        let bindings = match launch.bindings {
            Bindings::Unknown => OrchestratorBindings::Unknown,
            Bindings::TrustedFreshHostBindings => OrchestratorBindings::TrustedFreshHostBindings,
        };
        // This local controller protocol supplies an assertion only. Never expose
        // it as a model-controlled tool that can mint a trusted runtime context.
        let context = ExecutorContext::host_asserted(
            &launch.launch_id,
            &launch.readiness_identity,
            profile,
            bindings,
        );
        let source = match launch.kind {
            Kind::Script => ExecutorSource::Script {
                language: launch.language,
                source: &launch.source,
            },
            Kind::Orchestration => ExecutorSource::Orchestration {
                source: &launch.source,
            },
        };
        let input = FinalExecutorInput::new(
            &launch,
            &launch.call_id,
            source,
            launch.cwd.as_deref(),
            &context,
        );
        let prepared = input.prepare(
            AnalysisLimits::default(),
            Duration::from_millis(launch.wait_ms.unwrap_or(100)),
        );
        let dispatched = prepared.dispatch(|same| Release {
            source: same.source().source(),
            cwd: same.cwd(),
            launch_id: same.context().launch_id(),
            readiness_identity: same.context().readiness_identity(),
        });
        let record = serde_json::json!({
            "callId": launch.call_id,
            "launchId": launch.launch_id,
            "readinessIdentity": launch.readiness_identity,
            "attestation": "hostAssertionOnly",
            "sourceExecuted": false,
            "presentation": dispatched.presentation(),
            "release": dispatched.outcome(),
        });
        serde_json::to_writer(&mut output, &record)?;
        output.write_all(b"\n")?;
        output.flush()?;
    }
    Ok(())
}
