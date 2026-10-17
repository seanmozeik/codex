//! Complete syntax descriptions independent of runtime assumptions and authority.

mod dispatch;
mod input;
mod javascript;
mod model;
mod orchestration;
mod patch;
mod patch_model;
mod python;
mod python_legacy;
mod shell;

pub use input::CompleteSourceDescriptionInput;
pub use model::SourceDescription;
pub use model::SourceDescriptionDisposition;
pub use model::SourceDescriptionFallback;
pub use model::SourceReference;
pub use model::SourceReferenceBasis;
pub use model::SourceReferenceKind;
pub use patch_model::LiteralPatchPath;
pub use patch_model::PatchByteRange;
pub use patch_model::PatchChunk;
pub use patch_model::PatchEvidence;
pub use patch_model::PatchId;
pub use patch_model::PatchLine;
pub use patch_model::PatchLinePrefix;
pub use patch_model::PatchPathReference;
pub use patch_model::SourcePatch;
pub use patch_model::SourcePatchFile;

use std::time::Duration;
use std::time::Instant;
use tree_sitter::Node;
use tree_sitter::ParseOptions;
use tree_sitter::Parser;
use tree_sitter::Tree;

const MAX_NODES: usize = 4096;
const MAX_DEPTH: usize = 64;
const MAX_REFERENCES: usize = 256;
const MAX_RETAINED_BYTES: usize = 1_048_576;

#[derive(Clone, Copy)]
pub(crate) struct DescriptionBudget {
    started: Instant,
    elapsed: Duration,
}

impl DescriptionBudget {
    pub(crate) fn new(elapsed: Duration) -> Self {
        Self {
            started: Instant::now(),
            elapsed,
        }
    }

    pub(crate) fn check(self) -> Result<(), SourceDescriptionFallback> {
        if self.expired() {
            Err(SourceDescriptionFallback::TimeBudget)
        } else {
            Ok(())
        }
    }

    pub(crate) fn expired(self) -> bool {
        self.started.elapsed() >= self.elapsed
    }
}

pub(crate) fn parse(
    parser: &mut Parser,
    source: &str,
    budget: DescriptionBudget,
) -> Result<Tree, SourceDescriptionFallback> {
    budget.check()?;
    let bytes = source.as_bytes();
    let mut progress = |_: &tree_sitter::ParseState| budget.expired();
    let tree = parser.parse_with_options(
        &mut |offset, _| bytes.get(offset..).unwrap_or_default(),
        None,
        Some(ParseOptions::new().progress_callback(&mut progress)),
    );
    if tree.is_none() {
        parser.reset();
    }
    budget.check()?;
    let tree = tree.ok_or(SourceDescriptionFallback::UnsupportedSource)?;
    if source.contains('\0') || tree.root_node().has_error() {
        return Err(SourceDescriptionFallback::UnsupportedSource);
    }
    validate_size(tree.root_node(), budget)?;
    Ok(tree)
}

fn validate_size(
    root: Node<'_>,
    budget: DescriptionBudget,
) -> Result<(), SourceDescriptionFallback> {
    let mut cursor = root.walk();
    let mut nodes = 0usize;
    let mut depth = 0usize;
    loop {
        budget.check()?;
        nodes += 1;
        if nodes > MAX_NODES || depth > MAX_DEPTH {
            return Err(SourceDescriptionFallback::SourceLimit);
        }
        if cursor.goto_first_child() {
            depth += 1;
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return Ok(());
            }
            depth -= 1;
        }
    }
}

// The selective forecast collector reuses the same exact patch grammar. This
// adapter does not change complete-description admission or interpretation.
pub(crate) fn parse_selective_patch(
    payload: String,
    invocation: crate::Span,
    argument: crate::Span,
    patch_index: usize,
    budget: DescriptionBudget,
) -> Result<SourcePatch, SourceDescriptionFallback> {
    patch::scan(
        patch::LiteralPatchInvocation {
            payload,
            invocation,
            argument,
        },
        PatchId(patch_index),
        budget,
    )?
    .validate(budget)
}
