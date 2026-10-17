//! Source policies stay distinct from inferred host defaults.

use codex_protocol::source_description::SelectivePythonReadNewline;
use codex_protocol::source_description::SelectivePythonWriteNewline;

pub(super) fn read_label(newline: SelectivePythonReadNewline) -> &'static str {
    match newline {
        SelectivePythonReadNewline::UniversalTranslate => "translate universal newlines to LF",
        SelectivePythonReadNewline::Preserve => "preserve decoded characters",
    }
}

pub(super) fn write_label(newline: SelectivePythonWriteNewline) -> &'static str {
    match newline {
        SelectivePythonWriteNewline::RuntimeDefault => "runtime default (host separator required)",
        SelectivePythonWriteNewline::Preserve => "preserve characters",
        SelectivePythonWriteNewline::Cr => "translate LF to CR",
        SelectivePythonWriteNewline::CrLf => "translate LF to CRLF",
    }
}

#[cfg(test)]
mod tests;
