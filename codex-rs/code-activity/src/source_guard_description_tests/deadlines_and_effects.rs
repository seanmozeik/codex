use super::Analyzer;
use super::Arc;
use super::AtomicU8;
use super::Duration;
use super::EnvironmentAssumptions;
use super::GUARD_MAX_WAIT;
use super::Gate;
use super::GuardDisposition;
use super::GuardFallback;
use super::Instant;
use super::Job;
use super::Ordering;
use super::RecvTimeoutError;
use super::SOURCE;
use super::SourceAnalysisGuard;
use super::SourceDescription;
use super::SourceDescriptionGuardDisposition;
use super::State;
use super::TestResult;
use super::TestWorker;
use super::TryRecvError;
use super::WAIT;
use super::assert_eq;
use super::description;
use super::initialize_and_budget;
use super::script;
use super::sync_channel;

#[test]
fn host_deadline_includes_delay_before_shared_worker_admission() -> TestResult {
    let base = Instant::now();
    let (jobs, queued) = sync_channel(1);
    let guard = SourceAnalysisGuard {
        jobs,
        gate: Arc::new(Gate(AtomicU8::new(State::Ready as u8))),
    };
    let mut clock_calls = 0;
    let result = guard.describe_until_with(
        description(SOURCE)?,
        base + GUARD_MAX_WAIT,
        || {
            clock_calls += 1;
            base + Duration::from_millis(match clock_calls {
                1 => 80,
                2 => 90,
                _ => 95,
            })
        },
        |receiver, remaining| {
            // The original host deadline is 100 ms. The 80 ms already spent
            // outside the guard and the next 10 ms cannot become a new budget.
            assert_eq!(remaining, Duration::from_millis(10));
            let Job::Description {
                deadline, reply, ..
            } = queued
                .try_recv()
                .map_err(|_| RecvTimeoutError::Disconnected)?
            else {
                panic!("expected description job");
            };
            assert_eq!(deadline, base + GUARD_MAX_WAIT);
            guard.finish_test_reply(
                reply,
                Ok(SourceDescription::complete(
                    "reused-id".into(),
                    vec![],
                    vec![],
                    vec![],
                    vec![],
                )),
            );
            receiver.recv_timeout(WAIT)
        },
    );
    assert!(result.complete_description().is_some());
    assert_eq!(clock_calls, 3);
    Ok(())
}

#[test]
fn already_expired_host_deadline_never_takes_admission() -> TestResult {
    let base = Instant::now();
    let (jobs, queued) = sync_channel(1);
    let guard = SourceAnalysisGuard {
        jobs,
        gate: Arc::new(Gate(AtomicU8::new(State::Ready as u8))),
    };
    let expired = guard.describe_until_with(
        description(SOURCE)?,
        base,
        || base + Duration::from_millis(1),
        |_, _| panic!("expired host request must not wait for a worker"),
    );
    assert_eq!(
        expired.disposition(),
        SourceDescriptionGuardDisposition::Fallback(GuardFallback::Deadline)
    );
    assert!(matches!(queued.try_recv(), Err(TryRecvError::Empty)));
    assert_eq!(guard.gate.0.load(Ordering::Acquire), State::Ready as u8);
    Ok(())
}

#[test]
fn syntax_description_does_not_relax_unknown_runtime_effect_gate() -> TestResult {
    let base = Instant::now();
    let (retired, finished) = sync_channel(1);
    let guard = SourceAnalysisGuard::with_job_worker(
        TestWorker {
            analyzer: Analyzer::new()?,
            block: None,
        },
        move || {
            let _ = retired.try_send(());
        },
        move || base,
    );
    let described = guard.describe(description(SOURCE)?, GUARD_MAX_WAIT);
    finished.recv_timeout(WAIT)?;
    let value = described
        .complete_description()
        .ok_or("missing syntax description")?;
    assert!(!value.references().is_empty());
    let json = serde_json::to_value(&described)?;
    assert_eq!(json["type"], "completeSyntax");
    let effects = guard.analyze(
        script(EnvironmentAssumptions::PersistentOrUnknown)?,
        GUARD_MAX_WAIT,
    );
    assert_eq!(
        effects.disposition(),
        GuardDisposition::Fallback(GuardFallback::EnvironmentUnknown)
    );
    assert!(effects.ui_rows().is_empty());
    assert!(effects.diffs().is_empty());
    Ok(())
}

#[test]
fn description_queue_and_setup_reduce_original_budget_without_changing_source() -> TestResult {
    let base = Instant::now();
    let mut analyzer = None;
    let mut clock_calls = 0;
    let input = initialize_and_budget(
        description(SOURCE)?,
        base + GUARD_MAX_WAIT,
        &mut analyzer,
        || Some(()),
        || {
            clock_calls += 1;
            base + Duration::from_millis(if clock_calls == 1 { 20 } else { 80 })
        },
    )
    .map_err(|_| "unexpected budget rejection")?;
    assert_eq!(input.limits().elapsed(), Duration::from_millis(20));
    assert_eq!(input.limits().source_bytes(), 4096);
    assert_eq!(input.source(), SOURCE);
    assert_eq!(input.call_id().as_str(), "reused-id");
    Ok(())
}
