use super::DemoError;
use super::Result;
use serde::Deserialize;
use std::io::BufRead;
use std::io::Read;
use std::path::Path;

const MAX_LINE: u64 = 256 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Launch {
    pub call_id: String,
    pub command: Vec<String>,
    pub cwd: String,
    pub started_at_ms: i64,
}

impl Launch {
    pub fn validate(&self) -> Result<()> {
        if self.call_id.is_empty() || self.call_id.len() > 256 || self.started_at_ms < 0 {
            return Err(DemoError::Protocol(
                "invalid call identity or start timestamp",
            ));
        }
        if self.cwd.len() > 4096 || !Path::new(&self.cwd).is_absolute() || self.cwd.contains('\0') {
            return Err(DemoError::Protocol(
                "cwd must be a bounded absolute local path",
            ));
        }
        let [shell, flag, source] = self.command.as_slice() else {
            return Err(DemoError::Protocol(
                "expected shell, -c/-lc, and complete source",
            ));
        };
        if shell.is_empty()
            || shell.len() > 4096
            || !matches!(flag.as_str(), "-c" | "-lc")
            || source.len() > 32768
            || self.command.iter().any(|arg| arg.contains('\0'))
        {
            return Err(DemoError::Protocol("invalid or oversized command carrier"));
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Completion {
    pub exit_code: i32,
    pub output: String,
    pub duration_micros: u64,
    pub completed_at_ms: i64,
}

impl Completion {
    pub fn validate(&self, started_at_ms: i64) -> Result<()> {
        if self.completed_at_ms < started_at_ms
            || self.duration_micros > 300_000_000
            || self.output.len() > 65536
        {
            return Err(DemoError::Protocol("nonmonotonic or oversized completion"));
        }
        Ok(())
    }
}

pub(super) fn read_line(reader: &mut impl BufRead) -> Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    let count = reader.take(MAX_LINE + 1).read_until(b'\n', &mut bytes)?;
    if count == 0 {
        return Ok(None);
    }
    if count as u64 > MAX_LINE || !bytes.ends_with(b"\n") {
        return Err(DemoError::Protocol(
            "line exceeds byte cap or is not newline terminated",
        ));
    }
    Ok(Some(bytes))
}
