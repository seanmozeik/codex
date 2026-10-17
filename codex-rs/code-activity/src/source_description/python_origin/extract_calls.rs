use super::Mode;
use super::State;
use super::Text;
use super::Value;
use crate::source_description::python::origin::shared::Expr;
use crate::source_description::python::origin::shared::ExprKind;
use crate::source_description::python::origin::shared::Fallback;
use crate::source_description::python::origin::shared::ReferenceKind;
use crate::source_description::python::origin::shared::Span;

#[path = "extract_transforms.rs"]
mod transforms;

enum FileReceiver<'a> {
    Path(&'a str),
    Handle { path: &'a str, mode: Mode },
}

impl State {
    pub(super) fn call(
        &mut self,
        span: Span,
        function: &Expr,
        args: &[Expr],
        keywords: &[(String, Expr)],
        depth: usize,
    ) -> Result<Value, Fallback> {
        self.visit(depth)?;
        match &function.kind {
            ExprKind::Name(name) => self.named_call(span, name, args, keywords, depth),
            ExprKind::Attribute { value, name } => {
                let receiver = self.eval(value, depth + 1)?;
                match receiver {
                    Value::Path(path) => {
                        self.file_call(span, FileReceiver::Path(&path), name, args, keywords, depth)
                    }
                    Value::Handle { path, mode, .. } => self.file_call(
                        span,
                        FileReceiver::Handle { path: &path, mode },
                        name,
                        args,
                        keywords,
                        depth,
                    ),
                    Value::Text(text) if keywords.is_empty() => {
                        self.transform(span, text, name, args, depth)
                    }
                    _ => Err(Fallback::Unsupported),
                }
            }
            _ => Err(Fallback::Unsupported),
        }
    }

    fn named_call(
        &mut self,
        span: Span,
        name: &str,
        args: &[Expr],
        keywords: &[(String, Expr)],
        depth: usize,
    ) -> Result<Value, Fallback> {
        if self.constructor.as_deref() == Some(name) {
            let [path] = args else {
                return Err(Fallback::Unsupported);
            };
            if !keywords.is_empty() {
                return Err(Fallback::Unsupported);
            }
            let path = self.literal(path, depth + 1)?;
            if path.contains('\0') {
                return Err(Fallback::Unsupported);
            }
            self.emit(span, ReferenceKind::Path, Some(&path))?;
            return Ok(Value::Path(path));
        }
        if name == "open" || matches!(self.bindings.get(name), Some(Value::OpenName)) {
            self.encoding(keywords)?;
            let (path, mode) = match args {
                [path] => (path, Mode::Read),
                [path, mode] => {
                    let mode = match self.literal(mode, depth + 1)?.as_str() {
                        "r" => Mode::Read,
                        "w" => Mode::Write,
                        _ => return Err(Fallback::Unsupported),
                    };
                    (path, mode)
                }
                _ => return Err(Fallback::Unsupported),
            };
            let path = self.literal(path, depth + 1)?;
            if path.contains('\0') {
                return Err(Fallback::Unsupported);
            }
            self.emit(span, ReferenceKind::Open, Some(&path))?;
            return Ok(Value::Handle {
                path,
                mode,
                scoped: false,
            });
        }
        if !keywords.is_empty() {
            return Err(Fallback::Unsupported);
        }
        match name {
            "print" if !args.is_empty() => {
                for arg in args {
                    if !self.eval(arg, depth + 1)?.data() {
                        return Err(Fallback::Unsupported);
                    }
                }
                self.emit(span, ReferenceKind::Output, Some("print"))?;
                Ok(Value::Discard)
            }
            "len" => {
                let [arg] = args else {
                    return Err(Fallback::Unsupported);
                };
                let path = match self.eval(arg, depth + 1)? {
                    Value::Text(text) => text.path().map(str::to_owned),
                    Value::TextList(path) => path,
                    _ => return Err(Fallback::Unsupported),
                };
                self.emit(span, ReferenceKind::Transform, path.as_deref())?;
                Ok(Value::Integer(None))
            }
            _ => Err(Fallback::Unsupported),
        }
    }

    fn encoding(&mut self, keywords: &[(String, Expr)]) -> Result<(), Fallback> {
        match keywords {
            [] => Ok(()),
            [
                (
                    name,
                    Expr {
                        kind: ExprKind::Text(value),
                        ..
                    },
                ),
            ] if name == "encoding" && matches!(value.as_str(), "utf-8" | "utf8") => {
                self.charge(value.len())
            }
            _ => Err(Fallback::Unsupported),
        }
    }

    fn file_call(
        &mut self,
        span: Span,
        receiver: FileReceiver<'_>,
        name: &str,
        args: &[Expr],
        keywords: &[(String, Expr)],
        depth: usize,
    ) -> Result<Value, Fallback> {
        let (path, read, write) = match receiver {
            FileReceiver::Path(path) => {
                self.encoding(keywords)?;
                (path, name == "read_text", name == "write_text")
            }
            FileReceiver::Handle { path, mode } if keywords.is_empty() => (
                path,
                name == "read" && mode == Mode::Read,
                name == "write" && mode == Mode::Write,
            ),
            _ => return Err(Fallback::Unsupported),
        };
        if read && args.is_empty() {
            self.emit(span, ReferenceKind::Read, Some(path))?;
            return Ok(Value::Text(Text::Derived(Some(path.to_owned()))));
        }
        if write {
            let [arg] = args else {
                return Err(Fallback::Unsupported);
            };
            self.text(arg, depth + 1)?;
            self.emit(span, ReferenceKind::Write, Some(path))?;
            return Ok(Value::Discard);
        }
        Err(Fallback::Unsupported)
    }
}
