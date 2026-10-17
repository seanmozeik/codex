//! Literal text-I/O selectors, preserving omitted argument compatibility.

use super::Argument;
use super::Result;
use super::State;
use super::TextEncoding;
use crate::selective_edits::PythonAssumption;
use crate::selective_edits::PythonDependency;
use crate::selective_edits::PythonReadNewline;
use crate::selective_edits::PythonWriteNewline;

pub(super) fn encoding_argument(argument: &Argument) -> Option<TextEncoding> {
    if argument.direct_literal && matches!(argument.value.literal(), Some("utf-8" | "utf8")) {
        Some(TextEncoding::Utf8)
    } else {
        None
    }
}

pub(super) fn newline_argument(argument: &Argument) -> Option<PythonWriteNewline> {
    if argument.direct_none {
        return Some(PythonWriteNewline::RuntimeDefault);
    }
    if !argument.direct_literal {
        return None;
    }
    match argument.value.literal()? {
        "" | "\n" => Some(PythonWriteNewline::Preserve),
        "\r" => Some(PythonWriteNewline::Cr),
        "\r\n" => Some(PythonWriteNewline::CrLf),
        _ => None,
    }
}

pub(in crate::selective_edits::python::eval) fn read_newline(
    value: PythonWriteNewline,
) -> PythonReadNewline {
    match value {
        PythonWriteNewline::RuntimeDefault => PythonReadNewline::UniversalTranslate,
        PythonWriteNewline::Preserve | PythonWriteNewline::Cr | PythonWriteNewline::CrLf => {
            PythonReadNewline::Preserve
        }
    }
}

pub(super) fn method_options(
    arguments: &[Argument],
    positional: usize,
) -> Option<(TextEncoding, Option<PythonWriteNewline>)> {
    if arguments.len() < positional || arguments[..positional].iter().any(|arg| arg.name.is_some())
    {
        return None;
    }
    let mut encoding = TextEncoding::RuntimeDefault;
    let mut newline = None;
    for argument in &arguments[positional..] {
        match argument.name.as_deref()? {
            "encoding" => encoding = encoding_argument(argument)?,
            "newline" => newline = Some(newline_argument(argument)?),
            _ => return None,
        }
    }
    Some((encoding, newline))
}

impl State<'_> {
    pub(super) fn pathlib_newline(
        &mut self,
        newline: Option<PythonWriteNewline>,
        dependencies: &mut Vec<PythonDependency>,
    ) -> Result<()> {
        if newline.is_some() {
            if !self
                .plan
                .assumptions
                .contains(&PythonAssumption::PathlibNewlineSupport)
            {
                self.plan
                    .assumptions
                    .push(PythonAssumption::PathlibNewlineSupport);
            }
            self.dependency(
                dependencies,
                PythonDependency::Capability {
                    assumption: PythonAssumption::PathlibNewlineSupport,
                },
            )?;
        }
        Ok(())
    }
}
