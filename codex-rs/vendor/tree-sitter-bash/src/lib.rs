//! The pinned Bash grammar for source descriptions and conditional edit previews.
//!
//! Its scanner preserves an adjacent descriptor zero. Separate C symbols keep
//! ordinary analysis on the unchanged upstream grammar. See the crate README
//! for the upstream pin, scanner delta, and update procedure.

use tree_sitter_language::LanguageFn;

unsafe extern "C" {
    fn codex_tree_sitter_bash() -> *const ();
}

/// The isolated Bash grammar with corrected descriptor scanning.
// SAFETY: build.rs compiles the pinned generated parser and binds this symbol
// to its static TSLanguage using the tree-sitter language-function ABI.
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(codex_tree_sitter_bash) };
