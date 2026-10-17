//! Deterministic work and allocation ceilings; no wall-clock-dependent output.

use super::SnapshotPreviewError;
use super::SnapshotPreviewLimit;

/// Caller-adjustable ceilings, always bounded by the documented hard defaults.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct SnapshotPreviewLimits {
    /// Total input bytes and metadata; hard maximum 1 MiB and 256 read records.
    pub input_bytes: usize,
    /// Maximum bytes in any intermediate or output text; hard maximum 1 MiB.
    pub output_bytes: usize,
    /// Cumulative charged scan/allocation/evaluation units; hard maximum 32 MiB.
    pub work: usize,
    /// Evaluated recipe nodes; hard maximum 4096.
    pub nodes: usize,
    /// Evaluated recipe nesting; hard maximum 64.
    pub depth: usize,
    /// Unified-diff bytes; hard maximum 4 MiB.
    pub diff_bytes: usize,
}

impl Default for SnapshotPreviewLimits {
    fn default() -> Self {
        Self {
            input_bytes: 1_048_576,
            output_bytes: 1_048_576,
            work: 33_554_432,
            nodes: 4096,
            depth: 64,
            diff_bytes: 4_194_304,
        }
    }
}

pub(super) struct Budget {
    limits: SnapshotPreviewLimits,
    work: usize,
    nodes: usize,
}

impl Budget {
    pub(super) fn new(requested: SnapshotPreviewLimits) -> Self {
        let hard = SnapshotPreviewLimits::default();
        Self {
            limits: SnapshotPreviewLimits {
                input_bytes: requested.input_bytes.min(hard.input_bytes),
                output_bytes: requested.output_bytes.min(hard.output_bytes),
                work: requested.work.min(hard.work),
                nodes: requested.nodes.min(hard.nodes),
                depth: requested.depth.min(hard.depth),
                diff_bytes: requested.diff_bytes.min(hard.diff_bytes),
            },
            work: 0,
            nodes: 0,
        }
    }

    pub(super) fn input(&mut self, size: usize) -> Result<(), SnapshotPreviewError> {
        check(size, self.limits.input_bytes, SnapshotPreviewLimit::Input)?;
        self.work(size)
    }

    pub(super) fn work(&mut self, units: usize) -> Result<(), SnapshotPreviewError> {
        self.work = self.work.saturating_add(units);
        check(self.work, self.limits.work, SnapshotPreviewLimit::Work)
    }

    pub(super) fn enter(&mut self, depth: usize) -> Result<(), SnapshotPreviewError> {
        check(depth, self.limits.depth, SnapshotPreviewLimit::Depth)?;
        self.nodes = self.nodes.saturating_add(1);
        check(self.nodes, self.limits.nodes, SnapshotPreviewLimit::Nodes)?;
        self.work(1)
    }

    pub(super) fn output(&mut self, size: usize) -> Result<(), SnapshotPreviewError> {
        check(size, self.limits.output_bytes, SnapshotPreviewLimit::Output)?;
        self.work(size)
    }

    pub(super) fn copy(&mut self, text: &str) -> Result<String, SnapshotPreviewError> {
        self.output(text.len())?;
        Ok(text.to_owned())
    }

    pub(super) fn diff(&mut self, size: usize) -> Result<(), SnapshotPreviewError> {
        check(size, self.limits.diff_bytes, SnapshotPreviewLimit::Diff)
    }
}

fn check(
    size: usize,
    maximum: usize,
    limit: SnapshotPreviewLimit,
) -> Result<(), SnapshotPreviewError> {
    if size > maximum {
        Err(SnapshotPreviewError::ResourceLimit { limit })
    } else {
        Ok(())
    }
}
