//! Same-version selective presentation with the original source-only fallback.
//! Selection happens after one native job; it never re-admits or renews a budget.

use super::convert;
use super::fallback;
use super::supplier::AutomaticSnapshot;
use codex_code_activity::GuardFallback;
use codex_code_activity::GuardedSourcePresentation;
use codex_code_activity::SourcePresentationMode;
use codex_code_mode::CodeModeToolKind;
use codex_code_mode::ToolDefinition;
use codex_protocol::source_description::SourceDescription;
use codex_protocol::source_description::SourceDescriptionFallback;

pub(super) fn mode(tools: &[ToolDefinition]) -> SourcePresentationMode {
    if expected_bindings(tools) {
        SourcePresentationMode::ConditionalForecast
    } else {
        SourcePresentationMode::SourceOnly
    }
}

pub(super) fn description(
    result: Result<GuardedSourcePresentation, GuardFallback>,
    snapshots: &[AutomaticSnapshot],
) -> SourceDescription {
    let result = match result {
        Ok(result) => result,
        Err(reason) => return guard_fallback(reason),
    };
    choose(
        result
            .forecast()
            .map(|value| convert::forecast(value, snapshots)),
        convert::description(result.description()),
    )
}

fn choose(forecast: Option<SourceDescription>, complete: SourceDescription) -> SourceDescription {
    if let Some(candidate) = forecast
        && matches!(&candidate, SourceDescription::SelectiveFindings { edits, .. } if !edits.is_empty())
    {
        return candidate;
    }
    complete
}

fn expected_bindings(tools: &[ToolDefinition]) -> bool {
    [
        ("apply_patch", CodeModeToolKind::Freeform),
        ("exec_command", CodeModeToolKind::Function),
    ]
    .into_iter()
    .all(|(name, kind)| {
        let mut matching = tools.iter().filter(|tool| {
            tool.name == name
                || (tool.tool_name.name == name && tool.tool_name.is_default_namespace())
        });
        let Some(tool) = matching.next() else {
            return false;
        };
        matching.next().is_none()
            && tool.name == name
            && tool.tool_name.name == name
            && tool.tool_name.is_default_namespace()
            && tool.kind == kind
    })
}

fn guard_fallback(reason: GuardFallback) -> SourceDescription {
    fallback(match reason {
        GuardFallback::Busy => SourceDescriptionFallback::Busy,
        GuardFallback::Deadline => SourceDescriptionFallback::Deadline,
        GuardFallback::RequestTooLarge => SourceDescriptionFallback::SourceLimit,
        GuardFallback::Description(codex_code_activity::SourceDescriptionFallback::SourceLimit) => {
            SourceDescriptionFallback::SourceLimit
        }
        GuardFallback::Description(codex_code_activity::SourceDescriptionFallback::TimeBudget) => {
            SourceDescriptionFallback::Deadline
        }
        _ => SourceDescriptionFallback::Unavailable,
    })
}

#[cfg(test)]
mod tests;
