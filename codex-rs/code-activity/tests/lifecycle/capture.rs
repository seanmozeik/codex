//! capture behavior tests.
use super::*;
use pretty_assertions::assert_eq;

#[test]
fn prepare_captures_at_dispatch_and_keeps_original_text_out_of_events() -> eyre::Result<()> {
    let work = Workspace::new()?;
    work.file("a", "old while streaming")?;
    let mut stream = work.stream(Language::Python)?;
    stream.append("open('a','w').write('new')")?;
    work.file("a", "original at dispatch\n")?;
    let prepared = stream.finish()?.prepare(&mut work.provider()?, &[]);
    let json = serde_json::to_string(&prepared.activity())?;
    assert!(!json.contains("original at dispatch"));
    assert_eq!(prepared.activity().files.len(), 1);
    work.file("a", "new\n")?;
    let result = prepared.complete(&mut work.provider()?, exited(1));
    assert_eq!(result.execution.status, ExecutionStatus::Exited { code: 1 });
    assert_eq!(result.files[0].change, ObservedChange::Modified);
    let FileDiff::Compared { unified_diff, .. } = result.files[0]
        .diff
        .as_ref()
        .ok_or_else(|| eyre::eyre!("missing expected diff"))?
    else {
        panic!()
    };
    assert!(unified_diff.contains("-original at dispatch"));
    assert!(!unified_diff.contains("old while streaming"));
    Ok(())
}

#[test]
fn success_unchanged_failure_unchanged_and_failed_start_are_separate() -> eyre::Result<()> {
    let work = Workspace::new()?;
    work.file("a", "same")?;
    for status in [
        ExecutionStatus::Exited { code: 0 },
        ExecutionStatus::Exited { code: 1 },
        ExecutionStatus::FailedToStart,
        ExecutionStatus::Cancelled,
    ] {
        let result = work
            .prepare("open('a','w').write('same')")?
            .complete(&mut work.provider()?, ExecutionResult::new(status, ""));
        assert_eq!(result.files[0].change, ObservedChange::Unchanged);
        assert_eq!(result.observation_basis, ObservationBasis::CaptureInterval);
    }
    Ok(())
}

#[test]
fn empty_creation_deletion_and_read_observations_remain_distinct() -> eyre::Result<()> {
    let work = Workspace::new()?;
    work.file("deleted", "bye")?;
    work.file("read", "same")?;
    let prepared =
        work.prepare("open('created','w')\nopen('deleted').read()\nopen('read').read()")?;
    work.file("created", "")?;
    fs::remove_file(work.root.join("deleted"))?;
    let result = prepared.complete(&mut work.provider()?, exited(0));
    let changes: Vec<_> = result.files.iter().map(|f| &f.change).collect();
    assert_eq!(
        changes,
        vec![
            &ObservedChange::Created,
            &ObservedChange::Deleted,
            &ObservedChange::Unchanged
        ]
    );
    // No completed-read claim is made from an unchanged file.
    assert!(
        result
            .report
            .operations
            .iter()
            .all(|o| o.basis == Basis::StaticIntent)
    );
    Ok(())
}

#[test]
fn unsupported_paths_and_content_are_unknown_not_unchanged() -> eyre::Result<()> {
    let work = Workspace::new()?;
    fs::write(work.root.join("binary"), [0, 255])?;
    work.file("large", &"x".repeat(MAX_FILE_BYTES + 1))?;
    fs::create_dir(work.root.join("directory"))?;
    let result = work.prepare("open('binary').read()\nopen('large').read()\nopen('directory').read()\nopen('../outside').read()")?
        .complete(&mut work.provider()?, exited(0));
    assert!(
        result
            .files
            .iter()
            .all(|f| f.change == ObservedChange::Unknown && f.diff.is_none())
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlinks_are_not_followed() -> eyre::Result<()> {
    let work = Workspace::new()?;
    work.file("actual", "private")?;
    fs_err::os::unix::fs::symlink(work.root.join("actual"), work.root.join("alias"))?;
    let result = work
        .prepare("open('alias').read()")?
        .complete(&mut work.provider()?, exited(0));
    assert_eq!(result.files[0].change, ObservedChange::Unknown);
    Ok(())
}

#[test]
fn dynamic_targets_are_visible_and_host_paths_can_supply_observation() -> eyre::Result<()> {
    let work = Workspace::new()?;
    let mut stream = work.stream(Language::Python)?;
    stream.append("open(dynamic_path, 'w').write('x')")?;
    let prepared = stream
        .finish()?
        .prepare(&mut work.provider()?, &[work.root.join("actual")]);
    assert!(!prepared.activity().unobserved_operation_ids.is_empty());
    work.file("actual", "x")?;
    let result = prepared.complete(&mut work.provider()?, exited(0));
    assert_eq!(result.files[0].change, ObservedChange::Created);
    assert!(result.files[0].operation_ids.is_empty());
    Ok(())
}

#[test]
fn provider_limits_and_unknown_baselines_prevent_false_diffs() -> eyre::Result<()> {
    struct Provider;
    impl SnapshotProvider for Provider {
        fn capture(&mut self, _: &Path, max_bytes: usize) -> FileState {
            FileState::Text("x".repeat(max_bytes + 1))
        }
    }
    let mut stream = ActivityStream::new("s".into(), Language::Python, Some("/work".into()))?;
    stream.append("open('a').read()")?;
    let result = stream
        .finish()?
        .prepare(&mut Provider, &[])
        .complete(&mut Provider, exited(0));
    assert_eq!(result.files[0].change, ObservedChange::Unknown);
    assert!(result.files[0].diff.is_none());
    Ok(())
}

#[test]
fn capture_path_limit_is_reported_without_extra_reads() -> eyre::Result<()> {
    struct MissingProvider(usize);
    impl SnapshotProvider for MissingProvider {
        fn capture(&mut self, _: &Path, _: usize) -> FileState {
            self.0 += 1;
            FileState::Missing
        }
    }
    let mut provider = MissingProvider(0);
    let stream = ActivityStream::new("s".into(), Language::Python, None)?;
    let extras: Vec<PathBuf> = (0..300).map(|i| format!("/workspace/{i}").into()).collect();
    let prepared = stream.finish()?.prepare(&mut provider, &extras);
    assert_eq!(provider.0, MAX_CAPTURE_FILES);
    assert_eq!(prepared.activity().omitted_paths, 44);

    Ok(())
}

#[test]
fn capture_byte_limit_is_reported_without_extra_reads() -> eyre::Result<()> {
    struct FullProvider(usize);
    impl SnapshotProvider for FullProvider {
        fn capture(&mut self, _: &Path, max_bytes: usize) -> FileState {
            self.0 += 1;
            FileState::Text("x".repeat(max_bytes))
        }
    }
    let extras: Vec<PathBuf> = (0..10).map(|i| format!("/workspace/{i}").into()).collect();
    let mut provider = FullProvider(0);
    let stream = ActivityStream::new("s".into(), Language::Python, None)?;
    let prepared = stream.finish()?.prepare(&mut provider, &extras[..10]);
    assert_eq!(provider.0, MAX_CAPTURE_BYTES / MAX_FILE_BYTES);
    let unavailable = prepared
        .activity()
        .files
        .into_iter()
        .filter(|f| matches!(f.before, SnapshotSummary::Unavailable { .. }))
        .count();
    assert_eq!(unavailable, 2);
    Ok(())
}

#[test]
fn a_change_after_failed_start_is_not_attributed_to_the_tool() -> eyre::Result<()> {
    let work = Workspace::new()?;
    work.file("a", "before")?;
    let prepared = work.prepare("open('a','w')")?;
    work.file("a", "a different writer")?;
    let result = prepared.complete(
        &mut work.provider()?,
        ExecutionResult::new(ExecutionStatus::FailedToStart, "spawn failed"),
    );
    assert_eq!(result.execution.status, ExecutionStatus::FailedToStart);
    assert_eq!(result.files[0].change, ObservedChange::Modified);
    assert_eq!(result.observation_basis, ObservationBasis::CaptureInterval);
    assert_eq!(result.report.operations[0].basis, Basis::StaticIntent);
    Ok(())
}

#[test]
fn mutation_on_open_is_observed_without_a_write_call() -> eyre::Result<()> {
    let work = Workspace::new()?;
    work.file("a", "original\n")?;
    let prepared = work.prepare("open('a', 'w')")?;
    assert!(matches!(
        prepared.activity().report.operations[0].effect,
        Effect::FileOpen { .. }
    ));
    drop(fs::File::create(work.root.join("a"))?);
    let result = prepared.complete(&mut work.provider()?, exited(1));
    assert_eq!(result.files[0].change, ObservedChange::Modified);
    assert!(serde_json::to_string(&result)?.contains("-original"));
    Ok(())
}
