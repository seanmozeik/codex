//! Closed patch framing before the original pure parser, with shared accounting.

#[path = "patch_native.rs"]
mod native;
#[path = "patch_scan.rs"]
mod scan;

use super::DescriptionBudget;
use super::PatchEvidence;
use super::PatchId;
use super::SourceDescriptionFallback;
use super::SourcePatch;
use crate::Span;

pub(super) const MAX_PATCHES: usize = 8;
const MAX_PATCH_BYTES: usize = 16_320;
const MAX_FILES: usize = 32;
const MAX_CHUNKS: usize = 128;
const MAX_LINES: usize = 4096;
const UNSUPPORTED: SourceDescriptionFallback = SourceDescriptionFallback::UnsupportedSource;
const LIMIT: SourceDescriptionFallback = SourceDescriptionFallback::SourceLimit;
type Result<T> = std::result::Result<T, SourceDescriptionFallback>;

/// A cooked literal envelope awaiting complete patch and parent validation.
pub(super) struct LiteralPatchInvocation {
    pub(super) payload: String,
    pub(super) invocation: Span,
    pub(super) argument: Span,
}

#[derive(Default)]
pub(super) struct Counts {
    files: usize,
    chunks: usize,
    lines: usize,
}

impl Counts {
    pub(super) fn add(&mut self, other: &Self) -> Result<()> {
        self.files = self.files.checked_add(other.files).ok_or(LIMIT)?;
        self.chunks = self.chunks.checked_add(other.chunks).ok_or(LIMIT)?;
        self.lines = self.lines.checked_add(other.lines).ok_or(LIMIT)?;
        self.check()
    }

    fn check(&self) -> Result<()> {
        if self.files > MAX_FILES || self.chunks > MAX_CHUNKS || self.lines > MAX_LINES {
            return Err(LIMIT);
        }
        Ok(())
    }

    fn retained_bytes(&self, payload_bytes: usize) -> Result<usize> {
        let records = self
            .lines
            .checked_add(self.files)
            .and_then(|count| count.checked_add(self.chunks))
            .and_then(|count| count.checked_mul(256))
            .ok_or(LIMIT)?;
        payload_bytes
            .checked_mul(8)
            .and_then(|bytes| bytes.checked_add(records))
            .and_then(|bytes| bytes.checked_add(512))
            .ok_or(LIMIT)
    }
}

/// Scanner output stays private until native agreement and global reservation.
pub(super) struct ScannedPatch {
    preview: SourcePatch,
    pub(super) counts: Counts,
}

impl ScannedPatch {
    pub(super) fn retained_bytes(&self) -> Result<usize> {
        self.counts.retained_bytes(self.preview.payload.len())
    }

    pub(super) fn validate(self, budget: DescriptionBudget) -> Result<SourcePatch> {
        native::validate(&self.preview, budget)?;
        Ok(self.preview)
    }
}

pub(super) fn scan(
    invocation: LiteralPatchInvocation,
    id: PatchId,
    budget: DescriptionBudget,
) -> Result<ScannedPatch> {
    budget.check()?;
    if invocation.payload.len() > MAX_PATCH_BYTES {
        return Err(LIMIT);
    }
    let (files, counts) = scan::files(&invocation.payload, id, budget)?;
    Ok(ScannedPatch {
        preview: SourcePatch {
            id,
            invocation: invocation.invocation,
            argument: invocation.argument,
            payload: invocation.payload,
            files,
        },
        counts,
    })
}

fn text(patch: &SourcePatch, evidence: PatchEvidence) -> Result<&str> {
    if evidence.patch_id != patch.id {
        return Err(UNSUPPORTED);
    }
    patch
        .payload
        .get(evidence.range.start..evidence.range.end)
        .ok_or(UNSUPPORTED)
}
