use super::State;
use super::Text;
use super::Value;
use crate::source_description::python::origin::shared::BinaryOp;
use crate::source_description::python::origin::shared::Expr;
use crate::source_description::python::origin::shared::ExprKind;
use crate::source_description::python::origin::shared::Fallback;
use crate::source_description::python::origin::shared::Index;
use crate::source_description::python::origin::shared::ReferenceKind;
use crate::source_description::python::origin::shared::UnaryOp;

impl State {
    pub(super) fn eval(&mut self, expression: &Expr, depth: usize) -> Result<Value, Fallback> {
        self.visit(depth)?;
        let value = match &expression.kind {
            ExprKind::Text(text) => {
                if text.len() > 16_384 {
                    return Err(Fallback::Limit);
                }
                Value::Text(Text::Literal(text.clone()))
            }
            ExprKind::Integer(value) => Value::Integer(Some(*value)),
            ExprKind::Name(name) if name == "open" => Value::OpenName,
            ExprKind::Name(name) => self
                .bindings
                .get(name)
                .filter(|v| !matches!(v, Value::Closed))
                .cloned()
                .ok_or(Fallback::Unsupported)?,
            ExprKind::List(items) => {
                let mut texts = Vec::new();
                for item in items {
                    let Value::Text(text) = self.eval(item, depth + 1)? else {
                        return Err(Fallback::Unsupported);
                    };
                    texts.push(text);
                }
                Value::TextList(merge_many(&texts))
            }
            ExprKind::Call {
                function,
                args,
                keywords,
            } => self.call(expression.span, function, args, keywords, depth + 1)?,
            ExprKind::Binary { left, op, right } => {
                let left = self.eval(left, depth + 1)?;
                let right = self.eval(right, depth + 1)?;
                match (left, op, right) {
                    (Value::Text(left), BinaryOp::Add, Value::Text(right)) => {
                        Value::Text(Text::Derived(merge_text(&left, &right)))
                    }
                    (Value::TextList(left), BinaryOp::Add, Value::TextList(right)) => {
                        Value::TextList(same_path(left.as_deref(), right.as_deref()))
                    }
                    (Value::Integer(_), _, Value::Integer(_)) => Value::Integer(None),
                    _ => return Err(Fallback::Unsupported),
                }
            }
            ExprKind::Unary { op, value } => {
                let Value::Integer(value) = self.eval(value, depth + 1)? else {
                    return Err(Fallback::Unsupported);
                };
                Value::Integer(match op {
                    UnaryOp::Plus => value,
                    UnaryOp::Minus => value.and_then(i64::checked_neg),
                })
            }
            ExprKind::Subscript { value, index } => {
                let value = self.eval(value, depth + 1)?;
                self.index(index, depth + 1)?;
                let (result, path) = match value {
                    Value::Text(text) => (
                        Value::Text(Text::Derived(text.path().map(str::to_owned))),
                        text.path().map(str::to_owned),
                    ),
                    Value::TextList(path) => {
                        let result = match index {
                            Index::Item(_) => Value::Text(Text::Derived(path.clone())),
                            Index::Slice { .. } => Value::TextList(path.clone()),
                        };
                        (result, path)
                    }
                    _ => return Err(Fallback::Unsupported),
                };
                self.emit(expression.span, ReferenceKind::Transform, path.as_deref())?;
                result
            }
            ExprKind::Attribute { .. } => return Err(Fallback::Unsupported),
        };
        self.charge(64 + value.bytes())?;
        Ok(value)
    }

    fn index(&mut self, index: &Index, depth: usize) -> Result<(), Fallback> {
        match index {
            Index::Item(index) => {
                self.integer(index, depth)?;
            }
            Index::Slice { lower, upper, step } => {
                for bound in [lower, upper].into_iter().flatten() {
                    self.integer(bound, depth)?;
                }
                if let Some(step) = step {
                    // A known nonzero step has a closed signature. Derived
                    // integer step values could be zero and remain unsupported.
                    if self.integer(step, depth)?.is_none_or(|value| value == 0) {
                        return Err(Fallback::Unsupported);
                    }
                }
            }
        }
        Ok(())
    }

    pub(super) fn integer(&mut self, value: &Expr, depth: usize) -> Result<Option<i64>, Fallback> {
        match self.eval(value, depth)? {
            Value::Integer(value) => Ok(value),
            _ => Err(Fallback::Unsupported),
        }
    }

    pub(super) fn text(&mut self, value: &Expr, depth: usize) -> Result<Text, Fallback> {
        match self.eval(value, depth)? {
            Value::Text(value) => Ok(value),
            _ => Err(Fallback::Unsupported),
        }
    }

    pub(super) fn literal(&mut self, value: &Expr, depth: usize) -> Result<String, Fallback> {
        match self.text(value, depth)? {
            Text::Literal(value) => Ok(value),
            _ => Err(Fallback::Unsupported),
        }
    }
}

pub(super) fn same_path(left: Option<&str>, right: Option<&str>) -> Option<String> {
    left.filter(|path| Some(*path) == right).map(str::to_owned)
}

pub(super) fn merge_text(left: &Text, right: &Text) -> Option<String> {
    match (left, right) {
        (Text::Literal(_), Text::Derived(path)) | (Text::Derived(path), Text::Literal(_)) => {
            path.clone()
        }
        (Text::Derived(left), Text::Derived(right)) => same_path(left.as_deref(), right.as_deref()),
        (Text::Literal(_), Text::Literal(_)) => None,
    }
}

fn merge_many(texts: &[Text]) -> Option<String> {
    let mut origin: Option<Text> = None;
    for text in texts {
        origin = Some(match origin {
            None => text.clone(),
            Some(previous) => Text::Derived(merge_text(&previous, text)),
        });
    }
    origin.and_then(|text| text.path().map(str::to_owned))
}
