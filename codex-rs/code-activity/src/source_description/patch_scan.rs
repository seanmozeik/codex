//! Lossless canonical patch grammar; every non-LF byte belongs to a known line.

use super::Counts;
use super::DescriptionBudget;
use super::LIMIT;
use super::MAX_LINES;
use super::Result;
use super::UNSUPPORTED;
use crate::source_description::LiteralPatchPath;
use crate::source_description::PatchByteRange;
use crate::source_description::PatchChunk;
use crate::source_description::PatchEvidence;
use crate::source_description::PatchId;
use crate::source_description::PatchLine;
use crate::source_description::PatchLinePrefix;
use crate::source_description::PatchPathReference;
use crate::source_description::SourcePatchFile;

const ADD: &str = "*** Add File: ";
const DELETE: &str = "*** Delete File: ";
const UPDATE: &str = "*** Update File: ";
const MOVE: &str = "*** Move to: ";
const EOF: &str = "*** End of File";

#[derive(Clone, Copy)]
struct Line<'a> {
    text: &'a str,
    start: usize,
}

impl Line<'_> {
    fn evidence(self, id: PatchId) -> PatchEvidence {
        self.tail(id, 0)
    }

    fn tail(self, id: PatchId, prefix_bytes: usize) -> PatchEvidence {
        PatchEvidence {
            patch_id: id,
            range: PatchByteRange {
                start: self.start + prefix_bytes,
                end: self.start + self.text.len(),
            },
        }
    }

    fn is_file_header(self) -> bool {
        self.text.starts_with(ADD) || self.text.starts_with(DELETE) || self.text.starts_with(UPDATE)
    }
}

pub(super) fn files(
    payload: &str,
    id: PatchId,
    budget: DescriptionBudget,
) -> Result<(Vec<SourcePatchFile>, Counts)> {
    for character in payload.chars() {
        budget.check()?;
        if matches!(character, '\0' | '\r' | '\u{feff}') {
            return Err(UNSUPPORTED);
        }
    }
    let mut lines = Vec::new();
    let mut offset = 0;
    for raw in payload.split_inclusive('\n') {
        budget.check()?;
        if lines.len() >= MAX_LINES {
            return Err(LIMIT);
        }
        lines.push(Line {
            text: raw.strip_suffix('\n').unwrap_or(raw),
            start: offset,
        });
        offset += raw.len();
    }
    if lines
        .first()
        .is_none_or(|line| line.text != "*** Begin Patch")
        || lines.last().is_none_or(|line| line.text != "*** End Patch")
        || lines.len() < 3
    {
        return Err(UNSUPPORTED);
    }
    let end = lines.len() - 1;
    let mut scanner = Scanner {
        lines: &lines[..end],
        index: 1,
        id,
        budget,
        counts: Counts {
            lines: lines.len(),
            ..Counts::default()
        },
    };
    let mut files = Vec::new();
    while let Some(line) = scanner.current() {
        budget.check()?;
        scanner.counts.files += 1;
        scanner.counts.check()?;
        scanner.index += 1;
        let file = if line.text.starts_with(ADD) {
            SourcePatchFile::Add {
                path: path(line, ADD, id)?,
                lines: scanner.add_lines()?,
            }
        } else if line.text.starts_with(DELETE) {
            SourcePatchFile::Delete {
                path: path(line, DELETE, id)?,
            }
        } else if line.text.starts_with(UPDATE) {
            let path = path(line, UPDATE, id)?;
            let move_to = scanner.move_path()?;
            SourcePatchFile::Update {
                path,
                move_to,
                chunks: scanner.chunks()?,
            }
        } else {
            return Err(UNSUPPORTED);
        };
        files.push(file);
    }
    budget.check()?;
    Ok((files, scanner.counts))
}

struct Scanner<'a> {
    lines: &'a [Line<'a>],
    index: usize,
    id: PatchId,
    budget: DescriptionBudget,
    counts: Counts,
}

impl<'a> Scanner<'a> {
    fn current(&self) -> Option<Line<'a>> {
        self.lines.get(self.index).copied()
    }

    fn add_lines(&mut self) -> Result<Vec<PatchLine>> {
        let mut lines = Vec::new();
        while let Some(line) = self.current() {
            self.budget.check()?;
            if !line.text.starts_with('+') {
                break;
            }
            lines.push(content(line, self.id)?);
            self.index += 1;
        }
        if lines.is_empty() {
            return Err(UNSUPPORTED);
        }
        Ok(lines)
    }

    fn move_path(&mut self) -> Result<Option<PatchPathReference>> {
        let Some(line) = self.current().filter(|line| line.text.starts_with(MOVE)) else {
            return Ok(None);
        };
        let reference = path(line, MOVE, self.id)?;
        self.index += 1;
        Ok(Some(reference))
    }

    fn new_chunk(&mut self, marker: Option<PatchEvidence>) -> Result<PatchChunk> {
        self.counts.chunks += 1;
        self.counts.check()?;
        Ok(PatchChunk {
            marker,
            lines: vec![],
            end_of_file: None,
        })
    }

    fn chunks(&mut self) -> Result<Vec<PatchChunk>> {
        let mut chunks = Vec::new();
        let mut current: Option<PatchChunk> = None;
        while let Some(line) = self.current() {
            self.budget.check()?;
            if line.is_file_header() {
                break;
            }
            if line.text == "@@" || line.text.starts_with("@@ ") {
                if line.text != "@@" {
                    let anchor = line.text.strip_prefix("@@ ").ok_or(UNSUPPORTED)?;
                    if anchor.is_empty() || anchor.chars().next_back().is_some_and(whitespace) {
                        return Err(UNSUPPORTED);
                    }
                }
                if let Some(chunk) = current.take() {
                    push_chunk(&mut chunks, chunk)?;
                }
                current = Some(self.new_chunk(Some(line.evidence(self.id)))?);
            } else if line.text == EOF {
                let mut chunk = current.take().ok_or(UNSUPPORTED)?;
                chunk.end_of_file = Some(line.evidence(self.id));
                push_chunk(&mut chunks, chunk)?;
                self.index += 1;
                if self.current().is_some_and(|next| !next.is_file_header()) {
                    return Err(UNSUPPORTED);
                }
                break;
            } else {
                let item = content(line, self.id)?;
                if current.is_none() {
                    current = Some(self.new_chunk(None)?);
                }
                current.as_mut().ok_or(UNSUPPORTED)?.lines.push(item);
            }
            self.index += 1;
        }
        if let Some(chunk) = current {
            push_chunk(&mut chunks, chunk)?;
        }
        if chunks.is_empty() {
            return Err(UNSUPPORTED);
        }
        Ok(chunks)
    }
}

fn push_chunk(chunks: &mut Vec<PatchChunk>, chunk: PatchChunk) -> Result<()> {
    if chunk.lines.is_empty() {
        return Err(UNSUPPORTED);
    }
    chunks.push(chunk);
    Ok(())
}

fn content(line: Line<'_>, id: PatchId) -> Result<PatchLine> {
    let prefix = match line.text.as_bytes().first() {
        Some(b' ') => PatchLinePrefix::Context,
        Some(b'+') => PatchLinePrefix::Plus,
        Some(b'-') => PatchLinePrefix::Minus,
        _ => return Err(UNSUPPORTED),
    };
    Ok(PatchLine {
        prefix,
        evidence: line.evidence(id),
        text: line.tail(id, 1),
    })
}

fn path(line: Line<'_>, prefix: &str, id: PatchId) -> Result<PatchPathReference> {
    let spelling = line.text.strip_prefix(prefix).ok_or(UNSUPPORTED)?;
    if spelling.is_empty()
        || spelling.chars().next().is_some_and(whitespace)
        || spelling.chars().next_back().is_some_and(whitespace)
        || spelling.chars().any(|character| {
            matches!(character, '\u{0}'..='\u{1f}' | '\u{7f}'..='\u{9f}' | '\u{2028}' | '\u{2029}')
        })
    {
        return Err(UNSUPPORTED);
    }
    Ok(PatchPathReference {
        spelling: LiteralPatchPath(spelling.to_owned()),
        evidence: line.tail(id, prefix.len()),
    })
}

fn whitespace(character: char) -> bool {
    matches!(
        character,
        '\u{9}'..='\u{d}' | '\u{20}' | '\u{85}' | '\u{a0}' | '\u{1680}'
            | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}'
            | '\u{205f}' | '\u{3000}'
    )
}
