//! Authored source is data; callbacks record release without executing it.

use codex_code_activity::AnalysisLimits;
use codex_code_activity::ExecutorContext;
use codex_code_activity::ExecutorRuntimeProfile;
use codex_code_activity::ExecutorSource;
use codex_code_activity::FallbackReason;
use codex_code_activity::FinalExecutorInput;
use codex_code_activity::GUARD_MAX_CALL_ID_BYTES;
use codex_code_activity::GUARD_MAX_CWD_BYTES;
use codex_code_activity::GUARD_MAX_SOURCE_BYTES;
use codex_code_activity::GuardDisposition;
use codex_code_activity::GuardFallback;
use codex_code_activity::Language;
use codex_code_activity::OrchestrationFallback;
use codex_code_activity::OrchestratorBindings;
use pretty_assertions::assert_eq;
use std::cell::Cell;
use std::time::Duration;

#[derive(Debug, PartialEq, Eq)]
struct Launch {
    source: String,
    cwd: String,
    environment: Vec<(String, String)>,
    permission_marker: String,
}

fn launch(source: &str) -> Launch {
    Launch {
        source: source.into(),
        cwd: "/authored/workspace".into(),
        environment: vec![("AUTHORED_ENV".into(), "preserve-exact-value".into())],
        permission_marker: "existing-host-decision".into(),
    }
}

fn context(profile: ExecutorRuntimeProfile) -> ExecutorContext<'static> {
    ExecutorContext::host_asserted(
        "authored-launch-identity",
        "authored-ready-identity",
        profile,
        OrchestratorBindings::TrustedFreshHostBindings,
    )
}

#[test]
fn fallback_still_dispatches_same_launch_once_and_preserves_callback_error() {
    let original = launch("open('a').read()");
    let context = context(ExecutorRuntimeProfile::Unknown);
    let calls = Cell::new(0);
    let input = FinalExecutorInput::new(
        &original,
        "call",
        ExecutorSource::Script {
            language: Language::Python,
            source: &original.source,
        },
        Some(&original.cwd),
        &context,
    );
    assert_eq!(calls.get(), 0);
    let prepared = input.prepare(AnalysisLimits::default(), Duration::from_millis(100));
    assert_eq!(calls.get(), 0);
    assert_eq!(
        prepared.presentation().disposition(),
        GuardDisposition::Fallback(GuardFallback::EnvironmentUnknown)
    );
    assert!(prepared.presentation().script_rows().is_empty());
    assert!(prepared.presentation().groups().is_empty());
    let dispatched = prepared.dispatch(|same| {
        calls.set(calls.get() + 1);
        assert!(std::ptr::eq(same.original(), &original));
        assert!(std::ptr::eq(same.context(), &context));
        assert_eq!(same.source().source().as_ptr(), original.source.as_ptr());
        assert_eq!(same.cwd(), Some(original.cwd.as_str()));
        assert_eq!(same.context().launch_id(), "authored-launch-identity");
        assert_eq!(
            same.context().readiness_identity(),
            "authored-ready-identity"
        );
        assert_eq!(same.original().environment, original.environment);
        assert_eq!(same.original().permission_marker, "existing-host-decision");
        Err::<(), _>("unchanged host dispatch error")
    });
    assert_eq!(calls.get(), 1);
    assert!(std::ptr::eq(dispatched.input().original(), &original));
    let (presentation, outcome) = dispatched.into_parts();
    assert_eq!(outcome, Err("unchanged host dispatch error"));
    assert_eq!(
        presentation.disposition(),
        GuardDisposition::Fallback(GuardFallback::EnvironmentUnknown)
    );
}

#[test]
fn byte_limits_reject_before_copy_but_keep_raw_input_available_for_dispatch() {
    let original = launch("open('a').read()");
    let context = context(ExecutorRuntimeProfile::OrdinaryFresh);
    let huge_source = "x".repeat(GUARD_MAX_SOURCE_BYTES + 1);
    let huge_id = "i".repeat(GUARD_MAX_CALL_ID_BYTES + 1);
    let huge_cwd = "c".repeat(GUARD_MAX_CWD_BYTES + 1);
    for (id, source, cwd) in [
        ("call", huge_source.as_str(), None),
        (huge_id.as_str(), original.source.as_str(), None),
        ("call", original.source.as_str(), Some(huge_cwd.as_str())),
    ] {
        let prepared = FinalExecutorInput::new(
            &original,
            id,
            ExecutorSource::Script {
                language: Language::Python,
                source,
            },
            cwd,
            &context,
        )
        .prepare(AnalysisLimits::default(), Duration::from_millis(100));
        assert_eq!(
            prepared.presentation().disposition(),
            GuardDisposition::Fallback(GuardFallback::RequestTooLarge)
        );
        let dispatched = prepared.dispatch(|same| {
            assert_eq!(same.call_id(), id);
            assert_eq!(same.source().source(), source);
            assert_eq!(same.cwd(), cwd);
            assert!(std::ptr::eq(same.original(), &original));
            37
        });
        assert_eq!(*dispatched.outcome(), 37);
    }
}

#[test]
fn empty_readiness_and_profile_carrier_mismatch_cannot_enable_rich_rows() {
    let original = launch("cat a");
    let ready = context(ExecutorRuntimeProfile::PreloadedPathlib);
    let unready = ExecutorContext::host_asserted(
        "launch",
        "",
        ExecutorRuntimeProfile::OrdinaryFresh,
        OrchestratorBindings::TrustedFreshHostBindings,
    );
    for (context, source) in [
        (
            &ready,
            ExecutorSource::Script {
                language: Language::Shell,
                source: "cat a",
            },
        ),
        (
            &ready,
            ExecutorSource::Orchestration {
                source: "await tools.exec_command({cmd:'cat a'})",
            },
        ),
        (
            &unready,
            ExecutorSource::Script {
                language: Language::Python,
                source: "open('a').read()",
            },
        ),
    ] {
        let prepared = FinalExecutorInput::new(&original, "call", source, None, context)
            .prepare(AnalysisLimits::default(), Duration::from_millis(100));
        assert_eq!(
            prepared.presentation().disposition(),
            GuardDisposition::Fallback(GuardFallback::EnvironmentUnknown)
        );
        assert!(prepared.presentation().script_rows().is_empty());
        assert!(prepared.presentation().groups().is_empty());
    }
}

#[test]
fn zero_wait_keeps_dispatch_and_never_presents_a_partial_result() {
    let original = launch("open('a').read()");
    let context = context(ExecutorRuntimeProfile::OrdinaryFresh);
    let prepared = FinalExecutorInput::new(
        &original,
        "call",
        ExecutorSource::Script {
            language: Language::Python,
            source: &original.source,
        },
        None,
        &context,
    )
    .prepare(AnalysisLimits::default(), Duration::ZERO);
    assert_eq!(
        prepared.presentation().disposition(),
        GuardDisposition::Fallback(GuardFallback::Deadline)
    );
    assert!(prepared.presentation().script_rows().is_empty());
    let dispatched = prepared.dispatch(|same| same.source().source());
    assert_eq!(*dispatched.outcome(), original.source);
}

// Keep worker-admitting checks serial in this integration-test process. Tests
// above reject before admission; production busy behavior is covered by guard tests.
#[test]
fn complete_source_profiles_and_envelopes_are_atomic_before_recorded_release() {
    let ordinary = context(ExecutorRuntimeProfile::OrdinaryFresh);
    let pathlib = context(ExecutorRuntimeProfile::PreloadedPathlib);
    for (context, source, expected) in [
        (
            &ordinary,
            ExecutorSource::Script {
                language: Language::Python,
                source: "open('a').read()",
            },
            GuardDisposition::Eligible,
        ),
        (
            &ordinary,
            ExecutorSource::Script {
                language: Language::Python,
                source: "open('a').read(); unknown()",
            },
            GuardDisposition::Fallback(GuardFallback::Source(FallbackReason::UnresolvedSource)),
        ),
        (
            &ordinary,
            ExecutorSource::Script {
                language: Language::Python,
                source: "from pathlib import Path\nPath('a').write_text('x')",
            },
            GuardDisposition::Fallback(GuardFallback::Source(FallbackReason::UnresolvedSource)),
        ),
        (
            &pathlib,
            ExecutorSource::Script {
                language: Language::Python,
                source: "from pathlib import Path\nPath('a').write_text('x')",
            },
            GuardDisposition::Eligible,
        ),
        (
            &pathlib,
            ExecutorSource::Script {
                language: Language::Python,
                source: "from pathlib import Path\nPath('a').write_text('x')\nunknown()",
            },
            GuardDisposition::Fallback(GuardFallback::Source(FallbackReason::UnresolvedSource)),
        ),
        (
            &ordinary,
            ExecutorSource::Orchestration {
                source: "await tools.exec_command({cmd:'cat a'})",
            },
            GuardDisposition::Eligible,
        ),
        (
            &ordinary,
            ExecutorSource::Orchestration {
                source: "await tools.exec_command({cmd:'cat a'}); await tools.exec_command({cmd:'unknown'})",
            },
            GuardDisposition::Fallback(GuardFallback::Orchestration(OrchestrationFallback::Child(
                FallbackReason::UnresolvedSource,
            ))),
        ),
    ] {
        let original = launch(source.source());
        let calls = Cell::new(0);
        let prepared =
            FinalExecutorInput::new(&original, "call", source, Some(&original.cwd), context)
                .prepare(AnalysisLimits::default(), Duration::from_millis(100));
        assert_eq!(calls.get(), 0);
        assert_eq!(
            prepared.presentation().disposition(),
            expected,
            "{source:?}"
        );
        if expected != GuardDisposition::Eligible {
            assert!(prepared.presentation().script_rows().is_empty());
            assert!(prepared.presentation().groups().is_empty());
        }
        let dispatched = prepared.dispatch(|same| {
            calls.set(calls.get() + 1);
            assert!(std::ptr::eq(same.original(), &original));
            assert!(std::ptr::eq(same.context(), context));
            same.source().source()
        });
        assert_eq!(calls.get(), 1);
        assert_eq!(*dispatched.outcome(), source.source());
        assert_eq!(dispatched.presentation().disposition(), expected);
    }
}
