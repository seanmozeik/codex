//! Source-literal open options and successful-open truncation.

use super::Argument;
use super::Kind;
use super::Mode;
use super::PythonBarrierReason;
use super::PythonDependency;
use super::PythonOperation;
use super::Result;
use super::State;
use super::TextEncoding;
use super::Value;
use super::target_weight;
use tree_sitter::Node;

impl State<'_> {
    pub(super) fn open(
        &mut self,
        node: Node<'_>,
        arguments: Vec<Argument>,
        mut dependencies: Vec<PythonDependency>,
    ) -> Result<Value> {
        let positional = arguments
            .iter()
            .take_while(|arg| arg.name.is_none())
            .count();
        if !(1..=2).contains(&positional) {
            return self.halt(node, PythonBarrierReason::UnsupportedOperation);
        }
        let mut mode = if positional == 2 {
            arguments[1].value.literal()
        } else {
            Some("r")
        };
        let mut has_mode = positional == 2;
        let mut encoding = TextEncoding::RuntimeDefault;
        let mut newline = None;
        for argument in &arguments[positional..] {
            match argument.name.as_deref() {
                Some("mode") if !has_mode => {
                    mode = argument.value.literal();
                    has_mode = true;
                }
                Some("encoding") => {
                    let Some(value) = super::options::encoding_argument(argument) else {
                        return self.halt(node, PythonBarrierReason::UnsupportedOperation);
                    };
                    encoding = value;
                }
                Some("newline") => {
                    let Some(value) = super::options::newline_argument(argument) else {
                        return self.halt(node, PythonBarrierReason::UnsupportedOperation);
                    };
                    newline = Some(value);
                }
                _ => return self.halt(node, PythonBarrierReason::UnsupportedOperation),
            }
        }
        let mode = match mode {
            Some("r") => Mode::Read,
            Some("w") => Mode::Write,
            Some("r+") if newline.is_some() && encoding == TextEncoding::Utf8 => Mode::Update,
            _ => return self.halt(node, PythonBarrierReason::UnsupportedOperation),
        };
        let target = self.target(node, &arguments[0].value, &mut dependencies)?;
        if mode == Mode::Write {
            self.guard.charge(target_weight(&target))?;
            let copied_dependencies = self.copy_dependencies(&dependencies)?;
            self.emit(
                node,
                target.clone(),
                PythonOperation::Truncate,
                copied_dependencies,
            )?;
        }
        self.condition(node)?;
        Ok(Value::new(
            Kind::Handle {
                target,
                mode,
                encoding,
                newline,
            },
            dependencies,
        ))
    }
}
