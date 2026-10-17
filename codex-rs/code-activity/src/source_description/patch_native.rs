//! Compare the lossless scanner's projection with the original pure patch parser.

use super::DescriptionBudget;
use super::Result;
use super::UNSUPPORTED;
use super::text;
use crate::source_description::PatchChunk;
use crate::source_description::PatchLine;
use crate::source_description::PatchLinePrefix;
use crate::source_description::SourcePatch;
use crate::source_description::SourcePatchFile;
use codex_apply_patch::Hunk;
use codex_apply_patch::StreamingPatchParser;
use codex_apply_patch::UpdateFileChunk;

pub(super) fn validate(patch: &SourcePatch, budget: DescriptionBudget) -> Result<()> {
    budget.check()?;
    let mut parser = StreamingPatchParser::default();
    // The original API clones accumulated hunks. Never stream it once per line.
    drop(parser.push_delta(&patch.payload).map_err(|_| UNSUPPORTED)?);
    budget.check()?;
    let hunks = parser.finish().map_err(|_| UNSUPPORTED)?;
    budget.check()?;
    if parser.environment_id().is_some() || hunks.len() != patch.files.len() {
        return Err(UNSUPPORTED);
    }
    for (file, hunk) in patch.files.iter().zip(&hunks) {
        budget.check()?;
        match (file, hunk) {
            (
                SourcePatchFile::Add { path, lines },
                Hunk::AddFile {
                    path: native,
                    contents,
                },
            ) if native.to_str() == Some(path.spelling.as_str()) => {
                added_text(patch, lines, contents, budget)?;
            }
            (SourcePatchFile::Delete { path }, Hunk::DeleteFile { path: native })
                if native.to_str() == Some(path.spelling.as_str()) => {}
            (
                SourcePatchFile::Update {
                    path,
                    move_to,
                    chunks,
                },
                Hunk::UpdateFile {
                    path: native,
                    move_path,
                    chunks: native_chunks,
                },
            ) if native.to_str() == Some(path.spelling.as_str())
                && move_path.as_ref().and_then(|path| path.to_str())
                    == move_to.as_ref().map(|path| path.spelling.as_str())
                && chunks.len() == native_chunks.len() =>
            {
                for (chunk, native) in chunks.iter().zip(native_chunks) {
                    update_chunk(patch, chunk, native, budget)?;
                }
            }
            _ => return Err(UNSUPPORTED),
        }
    }
    budget.check()
}

fn added_text(
    patch: &SourcePatch,
    lines: &[PatchLine],
    contents: &str,
    budget: DescriptionBudget,
) -> Result<()> {
    let mut rest = contents;
    for line in lines {
        budget.check()?;
        rest = rest
            .strip_prefix(text(patch, line.text)?)
            .and_then(|rest| rest.strip_prefix('\n'))
            .ok_or(UNSUPPORTED)?;
    }
    if !rest.is_empty() {
        return Err(UNSUPPORTED);
    }
    Ok(())
}

fn update_chunk(
    patch: &SourcePatch,
    chunk: &PatchChunk,
    native: &UpdateFileChunk,
    budget: DescriptionBudget,
) -> Result<()> {
    budget.check()?;
    let marker = chunk.marker.map(|marker| text(patch, marker)).transpose()?;
    let anchor = match marker {
        None | Some("@@") => None,
        Some(marker) => Some(marker.strip_prefix("@@ ").ok_or(UNSUPPORTED)?),
    };
    if native.change_context.as_deref() != anchor
        || native.is_end_of_file != chunk.end_of_file.is_some()
    {
        return Err(UNSUPPORTED);
    }
    let mut old = 0;
    let mut new = 0;
    let mut context = 0;
    for line in &chunk.lines {
        budget.check()?;
        let value = text(patch, line.text)?;
        if matches!(
            line.prefix,
            PatchLinePrefix::Context | PatchLinePrefix::Minus
        ) && native.old_lines.get(old).map(String::as_str) != Some(value)
        {
            return Err(UNSUPPORTED);
        }
        if matches!(
            line.prefix,
            PatchLinePrefix::Context | PatchLinePrefix::Plus
        ) && native.new_lines.get(new).map(String::as_str) != Some(value)
        {
            return Err(UNSUPPORTED);
        }
        match line.prefix {
            PatchLinePrefix::Context => {
                if native.context_line_indices.get(context) != Some(&(old, new)) {
                    return Err(UNSUPPORTED);
                }
                old += 1;
                new += 1;
                context += 1;
            }
            PatchLinePrefix::Plus => new += 1,
            PatchLinePrefix::Minus => old += 1,
        }
    }
    if old != native.old_lines.len()
        || new != native.new_lines.len()
        || context != native.context_line_indices.len()
    {
        return Err(UNSUPPORTED);
    }
    Ok(())
}
