//! Developer accounting checks for the separate C numeric-argument extension.

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
fn numeric_occurrences_accumulate_without_charging_quoted_numbers()
-> Result<(), SourceDescriptionFallback> {
    let result = plan("head -n 0007; tail -n -0; head -n '7'; tail -n \"7\"")?;
    assert_eq!(result.extension_events, 2);
    assert_eq!(result.extension_name_bytes, 0);
    assert_eq!(result.references.len(), 4);

    let repeated = plan("cat 7 7 7")?;
    assert_eq!(repeated.extension_events, 3);
    assert_eq!(repeated.references.len(), 1);
    Ok(())
}

#[test]
fn numeric_charges_add_to_existing_composition_charges() -> Result<(), SourceDescriptionFallback> {
    let result = plan("head -n 1 && tail -n 2 | head -n 3")?;
    // Three numeric occurrences, one list, and one pipeline.
    assert_eq!(result.extension_events, 5);
    assert_eq!(result.extension_name_bytes, 0);
    assert_eq!(result.references.len(), 5);
    Ok(())
}

#[test]
fn inherited_uv_selector_adds_one_event_only_for_bare_number()
-> Result<(), SourceDescriptionFallback> {
    let bare = plan("uv --python 3 run python -c ''")?;
    let quoted = plan("uv --python '3' run python -c ''")?;
    assert_eq!(bare.extension_events, 1);
    assert_eq!(quoted.extension_events, 0);
    assert_eq!(bare.references.len(), 1);
    assert_eq!(quoted.references.len(), 1);
    assert_eq!(bare.children.len(), 1);
    assert_eq!(quoted.children.len(), 1);
    assert!(bare.children[0].source.is_empty());
    assert!(quoted.children[0].source.is_empty());
    Ok(())
}

#[test]
fn descriptor_zero_is_not_charged_as_numeric_argv() -> Result<(), SourceDescriptionFallback> {
    for (source, expected_events) in [
        ("cat <file", 1),
        ("cat 0<file", 1),
        ("cat 0 <file", 2),
        ("cat 00 <file", 2),
        ("cat '0'<file", 1),
    ] {
        let result = plan(source)?;
        // Every case has one existing file-redirect event. Only a separated
        // direct number argv leaf adds C's one occurrence charge.
        assert_eq!(result.extension_events, expected_events, "{source}");
        assert_eq!(result.extension_name_bytes, 0, "{source}");
        assert_eq!(result.references.len(), 2, "{source}");
        assert!(result.children.is_empty(), "{source}");
    }
    Ok(())
}

#[test]
fn malformed_and_out_of_range_descriptors_never_reach_numeric_argv() {
    for source in ["cat 00<file", "cat 12<file"] {
        assert_eq!(
            plan(source).err(),
            Some(SourceDescriptionFallback::UnsupportedSource),
            "{source}"
        );
    }
}

#[test]
fn heredoc_zero_is_charged_only_when_it_is_an_argument() -> Result<(), SourceDescriptionFallback> {
    for (source, expected_events, expected_children) in [
        ("cat <<'E'\nα\nE\n", 1, 0),
        ("cat 0<<'E'\nα\nE\n", 1, 0),
        ("cat 0 <<'E'\nα\nE\n", 2, 0),
        ("cat '0'<<'E'\nα\nE\n", 1, 0),
        ("bash 0<<'E'\npwd\nE\n", 0, 1),
    ] {
        let result = plan(source)?;
        assert_eq!(result.extension_events, expected_events, "{source:?}");
        assert_eq!(result.extension_name_bytes, 0, "{source:?}");
        assert_eq!(result.children.len(), expected_children, "{source:?}");
    }
    Ok(())
}
