//! Closed sequential Python source descriptions; no runtime effects inferred.

#[path = "python_origin/mod.rs"]
mod origin;

use super::DescriptionBudget;
use super::SourceDescriptionFallback;
use super::SourceReference;
use super::SourceReferenceBasis;
use super::SourceReferenceKind;
use crate::SourceId;
use crate::Span;
use tree_sitter::Parser;

pub(crate) fn describe(
    parser: &mut Parser,
    source: &str,
    budget: DescriptionBudget,
) -> Result<Vec<SourceReference>, SourceDescriptionFallback> {
    // Preserve the production parser's full-CST node/depth checks. The copied
    // owned IR and extractor additionally account for their retained objects.
    let tree = super::parse(parser, source, budget)?;
    match super::python_legacy::describe(&tree, source, budget) {
        Ok(references) => return Ok(references),
        Err(SourceDescriptionFallback::UnsupportedSource) => {}
        Err(reason) => return Err(reason),
    }
    let limits = origin::Limits {
        deadline: budget
            .started
            .checked_add(budget.elapsed)
            .ok_or(SourceDescriptionFallback::SourceLimit)?,
        max_source_bytes: 32_768,
        max_nodes: super::MAX_NODES,
        max_depth: super::MAX_DEPTH,
        max_retained_bytes: super::MAX_RETAINED_BYTES,
        max_references: super::MAX_REFERENCES,
    };
    let module = origin::treesitter::lower(source, &tree, limits).map_err(map_error)?;
    let references = origin::extract::describe(&module, limits).map_err(map_error)?;
    let result = references
        .into_iter()
        .map(|reference| map_reference(source, reference))
        .collect::<Result<Vec<_>, _>>()?;
    budget.check()?;
    Ok(result)
}

fn map_reference(
    source: &str,
    reference: origin::Reference,
) -> Result<SourceReference, SourceDescriptionFallback> {
    let start = reference.span.start;
    let end = reference.span.end;
    if end <= start {
        return Err(SourceDescriptionFallback::UnsupportedSource);
    }
    let prefix = source
        .get(..start)
        .ok_or(SourceDescriptionFallback::UnsupportedSource)?;
    let through = source
        .get(..end)
        .ok_or(SourceDescriptionFallback::UnsupportedSource)?;
    Ok(SourceReference {
        basis: SourceReferenceBasis::SourceSyntax,
        kind: match reference.kind {
            origin::ReferenceKind::Import => SourceReferenceKind::ImportFrom,
            origin::ReferenceKind::Path => SourceReferenceKind::PathConstructorCall,
            origin::ReferenceKind::Open => SourceReferenceKind::OpenCall,
            origin::ReferenceKind::Read => SourceReferenceKind::ReadCall,
            origin::ReferenceKind::Replace => SourceReferenceKind::ReplaceCall,
            origin::ReferenceKind::Transform => SourceReferenceKind::TransformCall,
            origin::ReferenceKind::Write => SourceReferenceKind::WriteCall,
            origin::ReferenceKind::Output => SourceReferenceKind::OutputConsumer,
        },
        evidence: Span {
            source_id: SourceId(0),
            start_byte: start,
            end_byte: end,
            start_line: prefix.bytes().filter(|byte| *byte == b'\n').count() + 1,
            end_line: through.bytes().filter(|byte| *byte == b'\n').count() + 1,
            start_column: prefix
                .rfind('\n')
                .map_or(start + 1, |offset| start - offset),
        },
        literal: reference.literal,
    })
}

fn map_error(error: origin::Fallback) -> SourceDescriptionFallback {
    match error {
        origin::Fallback::Syntax | origin::Fallback::Unsupported => {
            SourceDescriptionFallback::UnsupportedSource
        }
        origin::Fallback::Limit => SourceDescriptionFallback::SourceLimit,
        origin::Fallback::Deadline => SourceDescriptionFallback::TimeBudget,
    }
}
