use super::State;
use super::Text;
use super::Value;
use crate::source_description::python::origin::shared::Expr;
use crate::source_description::python::origin::shared::Fallback;
use crate::source_description::python::origin::shared::ReferenceKind;
use crate::source_description::python::origin::shared::Span;

impl State {
    pub(super) fn transform(
        &mut self,
        span: Span,
        receiver: Text,
        name: &str,
        args: &[Expr],
        depth: usize,
    ) -> Result<Value, Fallback> {
        let path = receiver.path().map(str::to_owned);
        let mut kind = ReferenceKind::Transform;
        let result = match name {
            "replace" => {
                let (old, new) = match args {
                    [old, new] => (old, new),
                    [old, new, count] => {
                        if self
                            .integer(count, depth + 1)?
                            .is_none_or(|count| count < 0)
                        {
                            return Err(Fallback::Unsupported);
                        }
                        (old, new)
                    }
                    _ => return Err(Fallback::Unsupported),
                };
                self.literal(old, depth + 1)?;
                self.literal(new, depth + 1)?;
                kind = ReferenceKind::Replace;
                Value::Text(Text::Derived(path.clone()))
            }
            "strip" | "lstrip" | "rstrip" => {
                match args {
                    [] => {}
                    [chars] => {
                        self.literal(chars, depth + 1)?;
                    }
                    _ => return Err(Fallback::Unsupported),
                }
                Value::Text(Text::Derived(path.clone()))
            }
            "split" | "rsplit" => {
                match args {
                    [] => {}
                    [separator] => {
                        self.literal(separator, depth + 1)?;
                    }
                    [separator, count] => {
                        self.literal(separator, depth + 1)?;
                        self.integer(count, depth + 1)?;
                    }
                    _ => return Err(Fallback::Unsupported),
                }
                Value::TextList(path.clone())
            }
            "splitlines" if args.is_empty() => Value::TextList(path.clone()),
            "find" | "index" | "rfind" | "rindex" => {
                let Some((needle, bounds)) = args.split_first() else {
                    return Err(Fallback::Unsupported);
                };
                if bounds.len() > 2 {
                    return Err(Fallback::Unsupported);
                }
                self.literal(needle, depth + 1)?;
                for bound in bounds {
                    self.integer(bound, depth + 1)?;
                }
                Value::Integer(None)
            }
            "join" => {
                // Literal separator avoids merging a second dynamic origin.
                if !matches!(receiver, Text::Literal(_)) {
                    return Err(Fallback::Unsupported);
                }
                let [arg] = args else {
                    return Err(Fallback::Unsupported);
                };
                let Value::TextList(list_path) = self.eval(arg, depth + 1)? else {
                    return Err(Fallback::Unsupported);
                };
                self.emit(span, kind, list_path.as_deref())?;
                return Ok(Value::Text(Text::Derived(list_path)));
            }
            _ => return Err(Fallback::Unsupported),
        };
        self.emit(span, kind, path.as_deref())?;
        Ok(result)
    }
}
