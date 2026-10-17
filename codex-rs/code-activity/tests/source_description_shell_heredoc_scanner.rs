//! Original-byte parser regressions; submitted shell programs remain inert.

use codex_code_activity::ActivityError;
use pretty_assertions::assert_eq;
use tree_sitter::Node;
use tree_sitter::Parser;
use tree_sitter::Tree;

fn parse(source: &str, language: tree_sitter::Language) -> Result<Tree, ActivityError> {
    let mut parser = Parser::new();
    parser.set_language(&language)?;
    parser
        .parse(source, None)
        .ok_or(ActivityError::ParseInterrupted)
}

fn redirect(tree: &Tree) -> Result<Node<'_>, ActivityError> {
    tree.root_node()
        .named_child(0)
        .and_then(|statement| statement.child_by_field_name("redirect"))
        .ok_or(ActivityError::ParseInterrupted)
}

#[test]
fn adjacent_zero_has_a_real_descriptor_before_the_heredoc_arrow() -> Result<(), ActivityError> {
    for source in [
        "cat 0<<'DATA'\nα\nDATA\n",
        "bash 0<<'CODE'\npwd\nCODE\n",
        "bash 0<<'CODE' | cat\npwd\nCODE\n",
        "echo input | bash 0<<'CODE'\npwd\nCODE\n",
        "python 0<<'PY'\nopen('a').read()\nPY\n",
        "python 0<<'PY' | wc -l\n \t\nopen('a').read()\nPY\npwd",
    ] {
        let tree = parse(source, codex_tree_sitter_bash::LANGUAGE.into())?;
        assert!(
            !tree.root_node().has_error(),
            "{source:?}: {}",
            tree.root_node().to_sexp()
        );
        let doc = redirect(&tree)?;
        assert_eq!(doc.kind(), "heredoc_redirect");
        let descriptor = doc
            .child_by_field_name("descriptor")
            .ok_or(ActivityError::ParseInterrupted)?;
        assert_eq!(descriptor.kind(), "file_descriptor");
        assert_eq!(source.get(descriptor.byte_range()), Some("0"));
        assert_eq!(descriptor.start_byte(), doc.start_byte());
        let arrow = doc.child(1).ok_or(ActivityError::ParseInterrupted)?;
        assert_eq!(arrow.kind(), "<<");
        assert_eq!(source.get(arrow.byte_range()), Some("<<"));
        assert_eq!(descriptor.end_byte(), arrow.start_byte());
    }
    Ok(())
}

#[test]
fn ordinary_grammar_keeps_its_independent_pinned_scanner() -> Result<(), ActivityError> {
    let source = "python 0<<'PY'\nopen('a').read()\nPY\n";
    let ordinary = parse(source, tree_sitter_bash::LANGUAGE.into())?;
    let preview = parse(source, codex_tree_sitter_bash::LANGUAGE.into())?;
    assert!(ordinary.root_node().has_error());
    assert!(!preview.root_node().has_error());
    Ok(())
}

#[test]
fn separated_quoted_and_escaped_zeros_remain_argv() -> Result<(), ActivityError> {
    for spelling in ["0 ", "'0'", "\"0\"", "\\0"] {
        let source = format!("cat {spelling}<<'E'\ndata\nE\n");
        let tree = parse(&source, codex_tree_sitter_bash::LANGUAGE.into())?;
        assert!(!tree.root_node().has_error(), "{source:?}");
        assert!(redirect(&tree)?.child_by_field_name("descriptor").is_none());
        let command = tree
            .root_node()
            .named_child(0)
            .and_then(|statement| statement.child_by_field_name("body"))
            .ok_or(ActivityError::ParseInterrupted)?;
        let argument = command
            .child_by_field_name("argument")
            .ok_or(ActivityError::ParseInterrupted)?;
        assert_eq!(source.get(argument.byte_range()), Some(spelling.trim_end()));
    }
    Ok(())
}

#[test]
fn descriptor_scanning_retains_every_digit() -> Result<(), ActivityError> {
    for descriptor in ["0", "00", "1", "2", "3", "10", "012"] {
        for redirect_suffix in ["<file", ">file", "<<'E'\ndata\nE\n"] {
            let source = format!("cat {descriptor}{redirect_suffix}");
            let tree = parse(&source, codex_tree_sitter_bash::LANGUAGE.into())?;
            assert!(!tree.root_node().has_error(), "{source:?}");
            let token = redirect(&tree)?
                .child_by_field_name("descriptor")
                .ok_or(ActivityError::ParseInterrupted)?;
            assert_eq!(
                source.get(token.byte_range()),
                Some(descriptor),
                "{source:?}"
            );
        }
    }
    Ok(())
}

#[test]
fn positional_parameter_scanning_matches_the_upstream_grammar() -> Result<(), ActivityError> {
    for source in [
        "echo $0 $10 ${0} ${10}",
        "echo \"$0 $10 ${0} ${10}\"",
        "echo ${0:-default} ${10:-default}",
        "echo ${0%pattern} ${0/pattern/replacement}",
        "echo ${0:1} ${10:1}",
    ] {
        let ordinary = parse(source, tree_sitter_bash::LANGUAGE.into())?;
        let preview = parse(source, codex_tree_sitter_bash::LANGUAGE.into())?;
        assert!(!ordinary.root_node().has_error(), "{source:?}");
        assert!(!preview.root_node().has_error(), "{source:?}");
        assert_eq!(
            preview.root_node().to_sexp(),
            ordinary.root_node().to_sexp(),
            "{source:?}"
        );
    }
    Ok(())
}
