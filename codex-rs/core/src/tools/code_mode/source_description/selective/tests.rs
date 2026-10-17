use super::super::describe;
use super::choose;
use super::expected_bindings;
use super::mode;
use codex_code_activity::SourcePresentationMode;
use codex_code_mode::CodeModeToolKind;
use codex_code_mode::ToolDefinition;
use codex_protocol::ToolName;
use codex_protocol::source_description::SourceDescription;
use codex_protocol::source_description::SourceDescriptionFallback;
use pretty_assertions::assert_eq;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

fn definitions() -> Vec<ToolDefinition> {
    [
        ("apply_patch", CodeModeToolKind::Freeform),
        ("exec_command", CodeModeToolKind::Function),
    ]
    .into_iter()
    .map(|(name, kind)| ToolDefinition {
        name: name.to_owned(),
        tool_name: ToolName::plain(name),
        description: String::new(),
        kind,
        input_schema: None,
        input_schema_max_bytes: None,
        output_schema: None,
    })
    .collect()
}

#[test]
fn actual_tool_map_requires_both_expected_names_identities_and_kinds() {
    let tools = definitions();
    assert!(expected_bindings(&tools));
    for change in 0..7 {
        let mut changed = tools.clone();
        match change {
            0 => {
                changed.pop();
            }
            1 => changed[0].name = "renamed_patch".to_owned(),
            2 => changed[0].tool_name = ToolName::plain("replacement"),
            3 => changed[1].kind = CodeModeToolKind::Freeform,
            4 => changed[0].tool_name = ToolName::namespaced("mcp", "apply_patch"),
            5 => changed.push(changed[0].clone()),
            _ => {
                let mut alias = changed[0].clone();
                alias.name = "alias".to_owned();
                changed.push(alias);
            }
        }
        assert!(!expected_bindings(&changed), "change {change}");
    }
    assert_eq!(
        tools,
        definitions(),
        "checking never rewrites runtime definitions"
    );
}

#[test]
fn unavailable_binding_preserves_original_source_only_mode() {
    assert_eq!(mode(&[]), SourcePresentationMode::SourceOnly);
    assert_eq!(
        mode(&definitions()),
        SourcePresentationMode::ConditionalForecast
    );
}

#[tokio::test]
async fn cancelled_selective_request_never_takes_admission() {
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let admission = Arc::new(Semaphore::new(1));
    assert!(
        describe(
            "call",
            "source",
            &definitions(),
            &cancellation,
            Arc::clone(&admission),
            None,
        )
        .await
        .is_err()
    );
    assert_eq!(admission.available_permits(), 1);
}

#[tokio::test]
async fn busy_selective_request_preserves_the_shared_gate() {
    let admission = Arc::new(Semaphore::new(0));
    let result = describe(
        "call",
        "source",
        &definitions(),
        &CancellationToken::new(),
        admission,
        None,
    )
    .await
    .expect("busy fallback");
    assert_eq!(
        result,
        SourceDescription::Fallback {
            reason: SourceDescriptionFallback::Busy
        }
    );
}

#[test]
fn only_nonempty_presentable_forecasts_replace_the_original_source_row() {
    let forecast: SourceDescription = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../protocol/tests/fixtures/source_description_selective.json"
    )))
    .expect("shared authored source fixture");
    let forecast = forecast.into_presentable(|_| true);
    let complete = SourceDescription::CompleteSyntax {
        sources: vec![],
        references: vec![],
        patches: vec![],
    };
    assert_eq!(choose(Some(forecast.clone()), complete.clone()), forecast);
    let SourceDescription::SelectiveFindings {
        sources, barriers, ..
    } = forecast
    else {
        panic!("fixture must have retained edits");
    };
    assert_eq!(
        choose(
            Some(SourceDescription::SelectiveFindings {
                sources,
                edits: vec![],
                barriers
            }),
            complete.clone()
        ),
        complete
    );
    for reason in [
        SourceDescriptionFallback::Presentation,
        SourceDescriptionFallback::UnsupportedSource,
        SourceDescriptionFallback::SourceLimit,
    ] {
        assert_eq!(
            choose(
                Some(SourceDescription::Fallback { reason }),
                complete.clone()
            ),
            complete
        );
    }
    assert_eq!(choose(None, complete.clone()), complete);
}
