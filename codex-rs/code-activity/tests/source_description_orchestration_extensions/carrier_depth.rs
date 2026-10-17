use super::ActivityError;
use super::PATCH_CALL;
use super::SourceDescriptionDisposition;
use super::SourceDescriptionFallback;
use super::assert_complete;
use super::describe;

fn nested_shell_heredoc_source(depth: usize) -> Result<String, ActivityError> {
    let mut shell = "pwd\n".to_owned();
    for index in 0..depth {
        shell = format!("bash <<'D{index}'\n{shell}D{index}\n");
    }
    let command = serde_json::to_string(&shell)?;
    Ok(format!(
        "text(await tools.exec_command({{cmd:{command}}}));"
    ))
}

#[test]
fn seven_owned_heredocs_preserve_the_leaf_at_carrier_depth_eight() -> Result<(), ActivityError> {
    let source = nested_shell_heredoc_source(7)?;
    let description = describe(&source)?;
    assert_complete(&description, &source);
    assert_eq!(description.sources().len(), 9);
    assert_eq!(
        description.source_texts().last().map(String::as_str),
        Some("pwd\n")
    );
    for child in description.sources().iter().skip(2) {
        let parent = child.parent.as_ref().expect("heredoc has its owning span");
        let parent_text = &description.source_texts()[parent.source_id.index()];
        let child_text = &description.source_texts()[child.id.index()];
        assert_eq!(
            parent_text.get(parent.start_byte..parent.end_byte),
            Some(child_text.as_str())
        );
    }
    Ok(())
}

#[test]
fn excess_heredoc_carrier_depth_reports_source_limit_and_erases_enrichment()
-> Result<(), ActivityError> {
    for depth in [8, 9] {
        for prefix in [String::new(), format!("await {PATCH_CALL};")] {
            let source = format!("{prefix}{}", nested_shell_heredoc_source(depth)?);
            let description = describe(&source)?;
            assert_eq!(
                description.disposition(),
                SourceDescriptionDisposition::Fallback(SourceDescriptionFallback::SourceLimit),
                "{source}"
            );
            assert!(description.sources().is_empty(), "{source}");
            assert!(description.source_texts().is_empty(), "{source}");
            assert!(description.references().is_empty(), "{source}");
            assert!(description.patches().is_empty(), "{source}");
        }
    }
    Ok(())
}
