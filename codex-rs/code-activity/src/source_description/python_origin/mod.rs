//! Closed sequential Python source origins; no runtime identity or effects.

pub(super) mod extract;
mod literal;
mod shared;
pub(super) mod treesitter;

pub(super) use shared::Fallback;
pub(super) use shared::Limits;
pub(super) use shared::Reference;
pub(super) use shared::ReferenceKind;
