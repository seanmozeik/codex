//! Selective carrier recipes after a finite whole-module compatibility check.
//!
//! This module does not publish effects or decide whether a preceding child
//! completed normally. The selective dispatcher owns those dependencies.
//! Syntax compatibility is independent of host trust. Only the reader uses
//! an explicit host profile to model a patch-display consumer's completion.

mod reader;
mod syntax;

use super::input::SourceKind;
use crate::Span;
use crate::source_description::SourceDescriptionFallback;

pub(crate) use reader::extract;

type Result<T> = std::result::Result<T, SourceDescriptionFallback>;
const UNSUPPORTED: SourceDescriptionFallback = SourceDescriptionFallback::UnsupportedSource;
const MAX_ITEMS: usize = 256;

#[derive(Debug)]
pub(crate) struct CarrierPlan {
    pub(crate) events: Vec<CarrierEvent>,
}

#[derive(Debug)]
pub(crate) enum CarrierEvent {
    Child {
        kind: SourceKind,
        text: String,
        evidence: Span,
        cwd: Option<String>,
        invocation: Span,
    },
    Patch {
        payload: String,
        invocation: Span,
        argument: Span,
        cwd: Option<String>,
    },
    PatchDisplayCompleted {
        evidence: Span,
    },
    Opaque {
        evidence: Span,
        reason: CarrierBarrier,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CarrierBarrier {
    UnknownExecution,
    UnsupportedControl,
    UnresolvedDispatch,
    UnresolvedArguments,
    ContextMutation,
    ConcurrentExecution,
}

pub(crate) type OrchestrationPlan = CarrierPlan;
