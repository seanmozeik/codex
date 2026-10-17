//! Match caller-supplied identities before decoding any selected read.

use super::SnapshotEncoding;
use super::SnapshotEpoch;
use super::SnapshotPreviewError;
use super::SnapshotPreviewInput;
use super::SnapshotPreviewLimit;
use super::SnapshotReadBinding;
use super::SnapshotReadNewline;
use super::SnapshotWriteNewline;
use super::SourceReadId;
use super::budget::Budget;
use crate::SourceId;
use crate::Target;
use crate::selective_edits::PythonWriteNewline;
use crate::selective_edits::TextEncoding;
use std::collections::HashMap;

pub(super) struct ReadExpectation<'a> {
    pub(super) read: SourceReadId,
    pub(super) filesystem_epoch: SnapshotEpoch,
    pub(super) target: &'a Target,
    pub(super) encoding: TextEncoding,
    pub(super) newline: SnapshotReadNewline,
}

pub(super) struct Snapshots<'a> {
    pub(super) input: &'a SnapshotPreviewInput,
    pub(super) source_id: SourceId,
    pub(super) epoch: SnapshotEpoch,
    pub(super) used: Vec<SnapshotReadBinding>,
    reads: HashMap<SourceReadId, &'a super::TrustedReadSnapshot>,
}

impl<'a> Snapshots<'a> {
    pub(super) fn new(
        input: &'a SnapshotPreviewInput,
        source_id: SourceId,
        epoch: SnapshotEpoch,
        target: &Target,
        budget: &mut Budget,
    ) -> Result<Self, SnapshotPreviewError> {
        if input.reads.len() > 256 {
            return Err(SnapshotPreviewError::ResourceLimit {
                limit: SnapshotPreviewLimit::Input,
            });
        }
        let mut size = input
            .call_id
            .as_str()
            .len()
            .saturating_add(target_size(&input.before.binding.target))
            .saturating_add(input.before.bytes.as_ref().map_or(0, Vec::len));
        for read in &input.reads {
            size = size
                .saturating_add(read.bytes.len())
                .saturating_add(target_size(&read.binding.target))
                .saturating_add(128);
        }
        budget.input(size)?;
        let before = &input.before.binding;
        if before.source_id != source_id
            || before.filesystem_epoch != epoch
            || &before.target != target
            || !matches!(target, Target::Literal { .. })
        {
            return Err(SnapshotPreviewError::TargetBindingMismatch);
        }
        let mut reads = HashMap::with_capacity(input.reads.len());
        for read in &input.reads {
            if reads.insert(read.binding.read, read).is_some() {
                return Err(SnapshotPreviewError::DuplicateRead {
                    read: read.binding.read,
                });
            }
        }
        Ok(Self {
            input,
            source_id,
            epoch,
            used: Vec::new(),
            reads,
        })
    }

    pub(super) fn encoding(&self, encoding: TextEncoding) -> Result<(), SnapshotPreviewError> {
        match encoding {
            TextEncoding::Utf8 => Ok(()),
            TextEncoding::RuntimeDefault
                if self.input.text_profile.runtime_default_encoding
                    == Some(SnapshotEncoding::Utf8) =>
            {
                Ok(())
            }
            TextEncoding::RuntimeDefault => Err(SnapshotPreviewError::UnresolvedEncoding),
        }
    }

    pub(super) fn read(
        &mut self,
        expected: ReadExpectation<'_>,
        budget: &mut Budget,
    ) -> Result<String, SnapshotPreviewError> {
        let id = expected.read;
        let snapshot = self
            .reads
            .get(&id)
            .ok_or(SnapshotPreviewError::MissingRead { read: id })?;
        budget.work(target_size(expected.target))?;
        if id.source_id != self.source_id
            || expected.filesystem_epoch.get() > self.epoch.get()
            || snapshot.binding.filesystem_epoch != expected.filesystem_epoch
            || &snapshot.binding.target != expected.target
            || snapshot.binding.encoding != expected.encoding
            || snapshot.binding.newline != expected.newline
        {
            return Err(SnapshotPreviewError::ReadBindingMismatch { read: id });
        }
        self.encoding(expected.encoding)?;
        if matches!(
            expected.newline,
            SnapshotReadNewline::Unspecified | SnapshotReadNewline::SourceDeclared
        ) || (self.input.text_profile.read_newline != SnapshotReadNewline::SourceDeclared
            && self.input.text_profile.read_newline != expected.newline)
        {
            return Err(SnapshotPreviewError::NewlineProfileMismatch);
        }
        self.consistent(snapshot, budget)?;
        let text =
            std::str::from_utf8(&snapshot.bytes).map_err(|_| SnapshotPreviewError::InvalidUtf8)?;
        budget.work(self.used.len())?;
        if !self.used.iter().any(|binding| binding.read == id) {
            budget.work(target_size(&snapshot.binding.target))?;
            self.used.push(snapshot.binding.clone());
        }
        budget.output(text.len())?;
        if snapshot.binding.newline == SnapshotReadNewline::Preserve {
            return Ok(text.to_owned());
        }
        let mut result = String::with_capacity(text.len());
        let mut characters = text.chars().peekable();
        while let Some(character) = characters.next() {
            if character == '\r' {
                if characters.peek() == Some(&'\n') {
                    let _ = characters.next();
                }
                result.push('\n');
            } else {
                result.push(character);
            }
        }
        Ok(result)
    }

    fn consistent(
        &self,
        snapshot: &super::TrustedReadSnapshot,
        budget: &mut Budget,
    ) -> Result<(), SnapshotPreviewError> {
        let binding = &snapshot.binding;
        let before = &self.input.before;
        budget.work(target_size(&binding.target))?;
        if binding.filesystem_epoch == before.binding.filesystem_epoch
            && binding.target == before.binding.target
        {
            budget.work(snapshot.bytes.len())?;
            if before.bytes.as_ref() != Some(&snapshot.bytes) {
                return Err(SnapshotPreviewError::ConflictingSnapshots { read: binding.read });
            }
        }
        for other in &self.input.reads {
            budget.work(target_size(&other.binding.target).saturating_add(1))?;
            if other.binding.read.source_id == binding.read.source_id
                && other.binding.filesystem_epoch == binding.filesystem_epoch
                && other.binding.target == binding.target
            {
                budget.work(snapshot.bytes.len())?;
                if other.bytes != snapshot.bytes {
                    return Err(SnapshotPreviewError::ConflictingSnapshots { read: binding.read });
                }
            }
        }
        Ok(())
    }

    pub(super) fn python_write(
        &self,
        text: String,
        encoding: TextEncoding,
        newline: Option<PythonWriteNewline>,
        budget: &mut Budget,
    ) -> Result<String, SnapshotPreviewError> {
        self.encoding(encoding)?;
        let profile = self.input.text_profile.write_newline;
        let valid = match newline {
            None | Some(PythonWriteNewline::RuntimeDefault) => matches!(
                profile,
                SnapshotWriteNewline::Lf | SnapshotWriteNewline::CrLf
            ),
            Some(PythonWriteNewline::Preserve) => profile == SnapshotWriteNewline::Preserve,
            Some(PythonWriteNewline::Cr) => profile == SnapshotWriteNewline::Cr,
            Some(PythonWriteNewline::CrLf) => profile == SnapshotWriteNewline::CrLf,
        };
        if !valid {
            return Err(SnapshotPreviewError::NewlineProfileMismatch);
        }
        match profile {
            SnapshotWriteNewline::Lf | SnapshotWriteNewline::Preserve => Ok(text),
            SnapshotWriteNewline::Cr | SnapshotWriteNewline::CrLf => {
                budget.work(text.len())?;
                let size = if profile == SnapshotWriteNewline::CrLf {
                    text.len()
                        .saturating_add(text.bytes().filter(|byte| *byte == b'\n').count())
                } else {
                    text.len()
                };
                budget.output(size)?;
                let mut result = String::with_capacity(size);
                for character in text.chars() {
                    if character == '\n' {
                        result.push('\r');
                        if profile == SnapshotWriteNewline::CrLf {
                            result.push('\n');
                        }
                    } else {
                        result.push(character);
                    }
                }
                Ok(result)
            }
            SnapshotWriteNewline::Unspecified => Err(SnapshotPreviewError::NewlineProfileMismatch),
        }
    }
}

fn target_size(target: &Target) -> usize {
    match target {
        Target::Literal { path, cwd } => path
            .len()
            .saturating_add(cwd.as_ref().map_or(0, String::len)),
        Target::Unresolved => 0,
    }
}
