//! Tests for streaming revisions and observed filesystem changes.

use codex_code_activity::*;
use fs_err as fs;
use std::path::Path;
use std::path::PathBuf;

struct Workspace {
    root: PathBuf,
    _directory: tempfile::TempDir,
}
impl Workspace {
    fn new() -> eyre::Result<Self> {
        let directory = tempfile::tempdir()?;
        let root = fs::canonicalize(directory.path())?;
        Ok(Self {
            root,
            _directory: directory,
        })
    }
    fn stream(&self, language: Language) -> eyre::Result<ActivityStream> {
        Ok(ActivityStream::new(
            "call".into(),
            language,
            Some(
                self.root
                    .to_str()
                    .ok_or_else(|| eyre::eyre!("non-UTF-8 test path"))?
                    .into(),
            ),
        )?)
    }
    fn provider(&self) -> eyre::Result<LocalFiles> {
        Ok(LocalFiles::new(&self.root)?)
    }
    fn file(&self, name: &str, text: &str) -> eyre::Result<()> {
        fs::write(self.root.join(name), text)?;
        Ok(())
    }
    fn prepare(&self, source: &str) -> eyre::Result<PreparedCall> {
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
