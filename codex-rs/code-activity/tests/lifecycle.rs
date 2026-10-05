//! Tests for streaming revisions and observed filesystem changes.

use anyhow::Context;
use codex_code_activity::*;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

struct Workspace {
    root: PathBuf,
    _directory: tempfile::TempDir,
}
impl Workspace {
    fn new() -> anyhow::Result<Self> {
        let directory = tempfile::tempdir()?;
        let root = fs::canonicalize(directory.path())
            .with_context(|| format!("cannot canonicalize {}", directory.path().display()))?;
        Ok(Self {
            root,
            _directory: directory,
        })
    }
    fn stream(&self, language: Language) -> anyhow::Result<ActivityStream> {
        Ok(ActivityStream::new(
            "call".into(),
            language,
            Some(
                self.root
                    .to_str()
                    .ok_or_else(|| anyhow::anyhow!("non-UTF-8 test path"))?
                    .into(),
            ),
        )?)
    }
    fn provider(&self) -> anyhow::Result<LocalFiles> {
        Ok(LocalFiles::new(&self.root)?)
    }
    fn file(&self, name: &str, text: &str) -> anyhow::Result<()> {
        let path = self.root.join(name);
        fs::write(&path, text).with_context(|| format!("cannot write {}", path.display()))?;
        Ok(())
    }
    fn prepare(&self, source: &str) -> anyhow::Result<PreparedCall> {
        let mut stream = self.stream(Language::Python)?;
        stream.append(source)?;
        Ok(stream.finish()?.prepare(&mut self.provider()?, &[]))
    }
}
fn exited(code: i32) -> ExecutionResult {
    ExecutionResult::new(ExecutionStatus::Exited { code }, "")
}

#[path = "lifecycle/streaming.rs"]
mod streaming;

#[path = "lifecycle/capture.rs"]
mod capture;

#[path = "lifecycle/runtime.rs"]
mod runtime;
