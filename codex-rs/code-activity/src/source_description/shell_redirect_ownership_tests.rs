//! Accounting checks for exact final-atom redirect ownership.

use super::Plan;
use super::SourceDescriptionFallback;
use super::extract;
use crate::source_description::DescriptionBudget;
use pretty_assertions::assert_eq;
use std::time::Duration;
use tree_sitter::Parser;

fn plan(source: &str) -> Result<Plan, SourceDescriptionFallback> {
    let budget = DescriptionBudget::new(Duration::from_secs(1));
    let mut parser = Parser::new();
    parser
        .set_language(&codex_tree_sitter_bash::LANGUAGE.into())
        .map_err(|_| SourceDescriptionFallback::UnsupportedSource)?;
    let tree = crate::source_description::parse(&mut parser, source, budget)?;
    extract(tree.root_node(), source, budget)
}

#[test]
fn file_only_wrappers_add_no_event_beyond_composition_redirects_and_argv()
-> Result<(), SourceDescriptionFallback> {
    for (source, events, references) in [
        ("pwd | cat >out", 2, 4),
        ("pwd | cat <in >out 2>&1", 4, 6),
        ("pwd | cat 0<in", 2, 4),
        ("pwd | cat 0 <in", 3, 4),
        ("pwd | cat '0'<in", 2, 4),
        ("head -n 0040 | tail -n -2 >out", 4, 4),
        ("pwd && cat >out", 2, 4),
        ("pwd || cat >out", 2, 4),
        ("pwd && cat | tail -n 2 >>out", 4, 6),
        ("(pwd) | cat >out", 3, 6),
    ] {
        let result = plan(source)?;
        assert_eq!(result.extension_events, events, "{source}");
        assert_eq!(result.extension_name_bytes, 0, "{source}");
        assert_eq!(result.references.len(), references, "{source}");
        assert!(result.children.is_empty(), "{source}");
    }
    Ok(())
}

#[test]
fn generic_attachment_preserves_heredoc_code_and_data_charges()
-> Result<(), SourceDescriptionFallback> {
    let data = plan("echo input | cat <<'DATA'\nvalue\nDATA\n")?;
    assert_eq!(data.extension_events, 2);
    assert_eq!(data.extension_name_bytes, 0);
    assert_eq!(data.references.len(), 4);
    assert!(data.children.is_empty());

    let code = plan("echo input | bash <<'CODE'\npwd\nCODE\n")?;
    assert_eq!(code.extension_events, 1);
    assert_eq!(code.extension_name_bytes, 0);
    assert_eq!(code.references.len(), 3);
    assert_eq!(code.children.len(), 1);
    assert_eq!(code.children[0].source, "pwd\n");
    Ok(())
}
