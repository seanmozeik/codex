//! Capture preparation and endpoint reconciliation.

use crate::CompletedPhase;
use crate::Effect;
use crate::ExecutionResult;
use crate::FileDiff;
use crate::FileSnapshot;
use crate::FileState;
use crate::FinalCall;
use crate::MAX_CAPTURE_BYTES;
use crate::MAX_CAPTURE_FILES;
use crate::ObservationBasis;
use crate::OperationId;
use crate::PreparedPhase;
use crate::Report;
use crate::SnapshotProvider;
use crate::SnapshotSummary;
use crate::Target;
use crate::snapshot::bounded_capture;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;

#[derive(Debug)]
struct Baseline {
    path: PathBuf,
    operation_ids: Vec<OperationId>,
    before: FileState,
}

/// Holds original contents in memory. It does not launch or authorize a command.
///
/// The host must prepare immediately before dispatch and complete after all
/// relevant writers have stopped, using the same provider and environment.
///
/// Completion consumes the baseline, so it cannot be repeated:
///
/// ```compile_fail
/// use codex_code_activity::{ExecutionResult, ExecutionStatus, PreparedCall, SnapshotProvider};
/// fn complete_twice(prepared: PreparedCall, files: &mut impl SnapshotProvider) {
///     let result = ExecutionResult::new(ExecutionStatus::Unknown, "");
///     let _ = prepared.complete(files, result);
///     let _ = prepared.complete(files, ExecutionResult::new(ExecutionStatus::Unknown, ""));
/// }
/// ```
#[derive(Debug)]
#[must_use]
/// Captured original contents awaiting a host-supplied terminal result.
#[non_exhaustive]
pub struct PreparedCall {
    report: Report,
    baselines: Vec<Baseline>,
    unobserved_operation_ids: Vec<OperationId>,
    omitted_paths: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
/// Serializable metadata for a prepared call; original text is not included.
#[non_exhaustive]
pub struct PreparedActivity<'a> {
    /// Marker for this lifecycle phase.
    pub phase: PreparedPhase,
    /// Frozen final source report.
    pub report: &'a Report,
    /// File observations for known or host-supplied paths.
    pub files: Vec<PreparedFile<'a>>,
    /// File operations for which an absolute target could not be resolved.
    pub unobserved_operation_ids: &'a [OperationId],
    /// Number of additional or over-budget paths omitted from capture.
    pub omitted_paths: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
/// Summary of an original file and the operations that refer to it.
#[non_exhaustive]
pub struct PreparedFile<'a> {
    /// Absolute path in the execution environment.
    pub path: &'a Path,
    /// Operations in the frozen report that refer to this path.
    pub operation_ids: &'a [OperationId],
    /// Original capture summary.
    pub before: SnapshotSummary,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// Net content or existence difference between two captured endpoints.
#[non_exhaustive]
pub enum ObservedChange {
    /// Absent before and present after.
    Created,
    /// Present before and absent after.
    Deleted,
    /// Both endpoints exist and their text differs.
    Modified,
    /// Endpoint text and existence are equal.
    Unchanged,
    /// Evidence is insufficient for a more specific classification.
    Unknown,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
/// One file comparison linked to the frozen operation IDs.
#[non_exhaustive]
pub struct ObservedFile {
    /// Absolute path in the execution environment.
    pub path: PathBuf,
    /// Operations in the frozen report that refer to this path.
    pub operation_ids: Vec<OperationId>,
    /// Original capture summary.
    pub before: SnapshotSummary,
    /// Final capture summary.
    pub after: SnapshotSummary,
    /// Endpoint comparison, independent of the exit code.
    pub change: ObservedChange,
    /// Text diff when both endpoints were available.
    pub diff: Option<FileDiff>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
/// Frozen source intent, terminal execution result and endpoint observations.
#[non_exhaustive]
pub struct CompletedActivity {
    /// Marker for this lifecycle phase.
    pub phase: CompletedPhase,
    /// Frozen final source report.
    pub report: Report,
    /// Terminal tool outcome, not inferred from file changes.
    pub execution: ExecutionResult,
    /// Comparison evidence belongs to the capture interval, not a proved writer.
    /// Evidence scope; does not identify the writer.
    pub observation_basis: ObservationBasis,
    /// File observations for known or host-supplied paths.
    pub files: Vec<ObservedFile>,
    /// File operations for which an absolute target could not be resolved.
    pub unobserved_operation_ids: Vec<OperationId>,
    /// Number of additional or over-budget paths omitted from capture.
    pub omitted_paths: usize,
}

impl FinalCall {
    /// Add paths known to the host when the AST cannot resolve them. The host
    /// must know them before execution to obtain a valid original snapshot.
    pub fn prepare(
        self,
        provider: &mut impl SnapshotProvider,
        additional_paths: &[PathBuf],
    ) -> PreparedCall {
        let mut targets: BTreeMap<PathBuf, Vec<OperationId>> = BTreeMap::new();
        let mut unobserved_operation_ids = vec![];
        for operation in &self.report.operations {
            let target = match &operation.effect {
                Effect::FileOpen { target, .. }
                | Effect::FileRead { target }
                | Effect::FileDelete { target }
                | Effect::FileTruncate { target }
                | Effect::FileWrite { target, .. }
                | Effect::FileEdit { target, .. } => target,
                Effect::FileList { .. } | Effect::ProcessRun { .. } => continue,
            };
            match absolute_target(target) {
                Some(path) => targets.entry(path).or_default().push(operation.id),
                None => unobserved_operation_ids.push(operation.id),
            }
        }
        // Limit the caller's extras before allocating a large map.
        let extra_overflow = additional_paths.len().saturating_sub(MAX_CAPTURE_FILES);
        let mut rejected_extras = 0;
        for path in additional_paths.iter().take(MAX_CAPTURE_FILES) {
            if path.is_absolute() {
                targets.entry(path.clone()).or_default();
            } else {
                rejected_extras += 1;
            }
        }
        let omitted_paths =
            targets.len().saturating_sub(MAX_CAPTURE_FILES) + extra_overflow + rejected_extras;
        let mut remaining = MAX_CAPTURE_BYTES;
        let mut baselines = vec![];
        for (index, (path, operation_ids)) in targets.into_iter().enumerate() {
            if index >= MAX_CAPTURE_FILES {
                unobserved_operation_ids.extend(operation_ids);
                continue;
            }
            let before = bounded_capture(provider, &path, &mut remaining);
            baselines.push(Baseline {
                path,
                operation_ids,
                before,
            });
        }
        PreparedCall {
            report: self.report,
            baselines,
            unobserved_operation_ids,
            omitted_paths,
        }
    }
}

impl PreparedCall {
    /// Returns serializable baseline metadata without original contents.
    #[must_use]
    pub fn activity(&self) -> PreparedActivity<'_> {
        PreparedActivity {
            phase: PreparedPhase::Prepared,
            report: &self.report,
            files: self
                .baselines
                .iter()
                .map(|b| PreparedFile {
                    path: &b.path,
                    operation_ids: &b.operation_ids,
                    before: b.before.summary(),
                })
                .collect(),
            unobserved_operation_ids: &self.unobserved_operation_ids,
            omitted_paths: self.omitted_paths,
        }
    }

    /// Captures final endpoints after the host has stopped relevant writers.
    #[must_use]
    pub fn complete(
        self,
        provider: &mut impl SnapshotProvider,
        execution: ExecutionResult,
    ) -> CompletedActivity {
        let mut remaining = MAX_CAPTURE_BYTES;
        let files = self
            .baselines
            .into_iter()
            .map(|baseline| {
                let after = bounded_capture(provider, &baseline.path, &mut remaining);
                observe(baseline, after)
            })
            .collect();
        CompletedActivity {
            phase: CompletedPhase::Completed,
            report: self.report,
            execution,
            observation_basis: ObservationBasis::CaptureInterval,
            files,
            unobserved_operation_ids: self.unobserved_operation_ids,
            omitted_paths: self.omitted_paths,
        }
    }
}

fn absolute_target(target: &Target) -> Option<PathBuf> {
    let Target::Literal { path, cwd } = target else {
        return None;
    };
    let path = Path::new(path);
    if path.is_absolute() {
        return Some(path.into());
    }
    let cwd = Path::new(cwd.as_deref()?);
    cwd.is_absolute().then(|| cwd.join(path))
}

fn observe(baseline: Baseline, after: FileState) -> ObservedFile {
    let change = match (&baseline.before, &after) {
        (FileState::Missing, FileState::Text(_)) => ObservedChange::Created,
        (FileState::Text(_), FileState::Missing) => ObservedChange::Deleted,
        (FileState::Text(a), FileState::Text(b)) if a != b => ObservedChange::Modified,
        (FileState::Text(_), FileState::Text(_)) | (FileState::Missing, FileState::Missing) => {
            ObservedChange::Unchanged
        }
        _ => ObservedChange::Unknown,
    };
    let before_summary = baseline.before.summary();
    let after_summary = after.summary();
    let diff = if change == ObservedChange::Unknown {
        None
    } else {
        let text = |state| {
            if let FileState::Text(s) = state {
                Some(s)
            } else {
                None
            }
        };
        crate::diff::compare(vec![FileSnapshot {
            path: baseline.path.to_string_lossy().into_owned(),
            before: text(baseline.before),
            after: text(after),
        }])
        .pop()
    };
    ObservedFile {
        path: baseline.path,
        operation_ids: baseline.operation_ids,
        before: before_summary,
        after: after_summary,
        change,
        diff,
    }
}
