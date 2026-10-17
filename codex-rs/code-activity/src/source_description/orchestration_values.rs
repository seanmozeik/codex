//! Bounded display origins, without retaining or evaluating display data trees.

use super::MAX_ITEMS;
use super::Reader;
use super::Result;
use super::SourceDescriptionFallback;
use super::UNSUPPORTED;
use super::syntax;
use std::collections::BTreeSet;
use tree_sitter::Node;

#[derive(Clone, Copy)]
pub(super) enum ToolResultKind {
    Command,
    Patch,
    Poll,
}

#[derive(Clone, Copy, Default)]
pub(super) struct ResultKinds(u8);

impl ResultKinds {
    pub(super) fn one(kind: ToolResultKind) -> Self {
        Self(match kind {
            ToolResultKind::Command => 1,
            ToolResultKind::Patch => 2,
            ToolResultKind::Poll => 4,
        })
    }

    fn insert(&mut self, kind: ToolResultKind) {
        self.0 |= Self::one(kind).0;
    }

    fn supports_output_fields(self) -> bool {
        self.0 != 0 && self.0 & Self::one(ToolResultKind::Patch).0 == 0
    }
}

#[derive(Clone, Copy)]
pub(super) enum JoinKind {
    All,
    AllSettled,
}

#[derive(Clone, Copy)]
pub(super) struct JoinedResults {
    kind: JoinKind,
    elements: [Option<ToolResultKind>; MAX_ITEMS],
    len: usize,
}

impl JoinedResults {
    pub(super) fn new(kind: JoinKind) -> Self {
        Self {
            kind,
            elements: [None; MAX_ITEMS],
            len: 0,
        }
    }

    pub(super) fn push(&mut self, kind: ToolResultKind) -> Result<()> {
        let slot = self
            .elements
            .get_mut(self.len)
            .ok_or(SourceDescriptionFallback::SourceLimit)?;
        *slot = Some(kind);
        self.len += 1;
        Ok(())
    }

    fn wrap(self, kinds: ResultKinds) -> ValueOrigin {
        match self.kind {
            JoinKind::All => ValueOrigin::Result(kinds),
            JoinKind::AllSettled => ValueOrigin::Settled(kinds),
        }
    }

    fn index(self, index: u64) -> Result<ValueOrigin> {
        let index = usize::try_from(index).map_err(|_| UNSUPPORTED)?;
        let kind = self
            .elements
            .get(index)
            .copied()
            .flatten()
            .ok_or(UNSUPPORTED)?;
        Ok(self.wrap(ResultKinds::one(kind)))
    }

    pub(super) fn element(self) -> ValueOrigin {
        let mut kinds = ResultKinds::default();
        for kind in self.elements.iter().flatten() {
            kinds.insert(*kind);
        }
        self.wrap(kinds)
    }
}

#[derive(Clone, Copy)]
pub(super) enum ValueOrigin {
    Result(ResultKinds),
    Settled(ResultKinds),
    Joined(JoinedResults),
    Scalar,
    Data,
}

impl ValueOrigin {
    pub(super) fn joined(self) -> Result<JoinedResults> {
        match self {
            Self::Joined(joined) => Ok(joined),
            _ => Err(UNSUPPORTED),
        }
    }

    fn project(self, property: &str) -> Result<Self> {
        match self {
            Self::Result(kinds)
                if kinds.supports_output_fields()
                    && matches!(
                        property,
                        "output"
                            | "chunk_id"
                            | "wall_time_seconds"
                            | "exit_code"
                            | "session_id"
                            | "original_token_count"
                    ) =>
            {
                Ok(Self::Scalar)
            }
            Self::Settled(kinds) if kinds.0 != 0 => match property {
                "status" | "reason" => Ok(Self::Scalar),
                "value" => Ok(Self::Result(kinds)),
                _ => Err(UNSUPPORTED),
            },
            _ => Err(UNSUPPORTED),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct LocalBindings<'source> {
    pub element_name: &'source str,
    pub element_origin: ValueOrigin,
    pub index_name: Option<&'source str>,
}

impl LocalBindings<'_> {
    fn origin(self, name: &str) -> Option<ValueOrigin> {
        if name == self.element_name {
            Some(self.element_origin)
        } else if self.index_name == Some(name) {
            Some(ValueOrigin::Scalar)
        } else {
            None
        }
    }

    fn contains(self, name: &str) -> bool {
        name == self.element_name || self.index_name == Some(name)
    }
}

#[derive(Clone, Copy)]
pub(super) enum DisplayScope<'source> {
    Outer,
    Body(LocalBindings<'source>),
}

impl Reader<'_> {
    pub(super) fn await_statement(&mut self, mut node: Node<'_>) -> Result<()> {
        while node.kind() == "parenthesized_expression" {
            self.extension(1, 0)?;
            node = syntax::parenthesized(node, self.budget)?;
        }
        self.awaited(node)?;
        Ok(())
    }

    pub(super) fn display_value(
        &mut self,
        node: Node<'_>,
        scope: DisplayScope<'_>,
    ) -> Result<ValueOrigin> {
        self.budget.check()?;
        match node.kind() {
            "identifier" => {
                let name = syntax::identifier(node, self.source)?;
                if let DisplayScope::Body(locals) = scope
                    && let Some(origin) = locals.origin(name)
                {
                    return Ok(origin);
                }
                self.binding_origin(node)
            }
            "await_expression" if matches!(scope, DisplayScope::Outer) => self.awaited(node),
            "parenthesized_expression" => {
                self.extension(1, 0)?;
                self.display_value(syntax::parenthesized(node, self.budget)?, scope)
            }
            "null" | "true" | "false" => {
                exact_keyword(node, node.kind(), self.source)?;
                self.extension(1, 0)?;
                Ok(ValueOrigin::Scalar)
            }
            "string" | "template_string" => {
                self.string(node)?;
                self.extension(1, 0)?;
                Ok(ValueOrigin::Scalar)
            }
            "number" | "unary_expression" => {
                syntax::signed_decimal(
                    node,
                    self.source,
                    9_007_199_254_740_991,
                    9_007_199_254_740_991,
                    self.budget,
                )?;
                self.extension(1, 0)?;
                Ok(ValueOrigin::Scalar)
            }
            "array" => {
                self.extension(1, 0)?;
                for child in syntax::sequence(node, "[", "]", self.budget)? {
                    self.display_value(child, scope)?;
                }
                Ok(ValueOrigin::Data)
            }
            "object" => self.display_record(node, scope),
            "member_expression" => {
                let (object, property) = syntax::member_parts(node, self.source, self.budget)?;
                self.extension(1, 0)?;
                self.display_value(object, scope)?.project(property)
            }
            "subscript_expression" => {
                if syntax::parts(node, self.budget)?.len() != 2 {
                    return Err(UNSUPPORTED);
                }
                syntax::punctuation(node, &["[", "]"], self.budget)?;
                let index = syntax::decimal_value(syntax::field(node, "index")?, self.source)?;
                self.extension(1, 0)?;
                self.display_value(syntax::field(node, "object")?, scope)?
                    .joined()?
                    .index(index)
            }
            "call_expression" => self.stringify(node, scope),
            _ => Err(UNSUPPORTED),
        }
    }

    fn display_record(&mut self, node: Node<'_>, scope: DisplayScope<'_>) -> Result<ValueOrigin> {
        self.extension(1, 0)?;
        let mut keys = BTreeSet::new();
        for pair in syntax::sequence(node, "{", "}", self.budget)? {
            if pair.kind() != "pair" || syntax::parts(pair, self.budget)?.len() != 2 {
                return Err(UNSUPPORTED);
            }
            syntax::punctuation(pair, &[":"], self.budget)?;
            let key = syntax::field(pair, "key")?;
            let key = match key.kind() {
                "property_identifier"
                    if syntax::record_identifier(crate::text(key, self.source)) =>
                {
                    crate::text(key, self.source).to_owned()
                }
                "string" => syntax::decode(crate::text(key, self.source), self.budget)?,
                _ => return Err(UNSUPPORTED),
            };
            if matches!(key.as_str(), "__proto__" | "prototype" | "constructor")
                || !keys.insert(key)
            {
                return Err(UNSUPPORTED);
            }
            self.extension(1, 0)?;
            self.display_value(syntax::field(pair, "value")?, scope)?;
        }
        Ok(ValueOrigin::Data)
    }

    fn stringify(&mut self, node: Node<'_>, scope: DisplayScope<'_>) -> Result<ValueOrigin> {
        let (function, arguments) = syntax::call(node, self.budget)?;
        if syntax::member(function, self.source, self.budget)? != ("JSON", "stringify")
            || self.json_shadowed
            || matches!(scope, DisplayScope::Body(locals) if locals.contains("JSON"))
        {
            return Err(UNSUPPORTED);
        }
        let arguments = syntax::sequence(arguments, "(", ")", self.budget)?;
        let value = match arguments.as_slice() {
            [value] => *value,
            [value, replacer, indent] => {
                exact_keyword(*replacer, "null", self.source)?;
                if syntax::decimal_value(*indent, self.source)? > 10 {
                    return Err(UNSUPPORTED);
                }
                self.extension(2, 0)?;
                *value
            }
            _ => return Err(UNSUPPORTED),
        };
        self.extension(1, 0)?;
        self.display_value(value, scope)?;
        Ok(ValueOrigin::Scalar)
    }
}

fn exact_keyword(node: Node<'_>, expected: &str, source: &str) -> Result<()> {
    if node.kind() == expected && node.child_count() == 0 && crate::text(node, source) == expected {
        Ok(())
    } else {
        Err(UNSUPPORTED)
    }
}
