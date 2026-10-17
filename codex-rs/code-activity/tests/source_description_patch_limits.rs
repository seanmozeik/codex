//! Inclusive published bounds for the complete literal-patch description.

use codex_code_activity::ActivityError;
use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::CompleteSourceDescriptionInput;
use codex_code_activity::Language;
use codex_code_activity::SourceDescription;
use codex_code_activity::SourceDescriptionDisposition;
use codex_code_activity::SourceDescriptionFallback;
use pretty_assertions::assert_eq;
use std::time::Duration;

const START: &str = "*** Begin Patch\n*** Add File: a\n+";
const END: &str = "\n*** End Patch";

fn describe(source: &str) -> Result<SourceDescription, ActivityError> {
    let input = CompleteSourceDescriptionInput::from_source(
        "authored-patch-boundary".into(),
        Language::TypeScript,
        source,
        AnalysisLimits::new(32_768, Duration::from_secs(1)),
    )?;
    Ok(Analyzer::new()?.describe_complete(input))
}

fn template_call(payload: &str) -> String {
    format!("await tools.apply_patch(`{payload}`);")
}

fn payload_with_bytes(bytes: usize) -> String {
    format!(
        "{START}{}{END}",
        "a".repeat(bytes - START.len() - END.len())
    )
}

fn assert_limit(description: &SourceDescription) {
    assert_eq!(
        description.disposition(),
        SourceDescriptionDisposition::Fallback(SourceDescriptionFallback::SourceLimit)
    );
    assert!(description.sources().is_empty());
    assert!(description.source_texts().is_empty());
    assert!(description.references().is_empty());
    assert!(description.patches().is_empty());
}

#[test]
fn individual_cooked_patch_byte_limit_is_inclusive() -> Result<(), ActivityError> {
    let at_limit = payload_with_bytes(16_320);
    let description = describe(&template_call(&at_limit))?;
    assert_eq!(
        description.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    assert_eq!(description.patches()[0].payload.len(), 16_320);
    assert_limit(&describe(&template_call(&payload_with_bytes(16_321)))?);
    Ok(())
}

#[test]
fn aggregate_decoded_bytes_charge_every_occurrence_and_the_outer_source()
-> Result<(), ActivityError> {
    let payload = payload_with_bytes(8_000);
    let call = template_call(&payload);
    let mut source = format!("{call}{call}/**/");
    let padding = 32_768 - source.len() - 2 * payload.len();
    source.insert_str(source.len() - 2, &"x".repeat(padding));
    assert_eq!(source.len() + 2 * payload.len(), 32_768);
    let complete = describe(&source)?;
    assert_eq!(
        complete.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    assert_eq!(complete.patches().len(), 2);
    assert_eq!(
        complete
            .source_texts()
            .iter()
            .map(String::len)
            .sum::<usize>(),
        source.len()
    );
    source.insert(source.len() - 2, 'x');
    assert_limit(&describe(&source)?);
    Ok(())
}

fn deletes(count: usize) -> String {
    format!(
        "*** Begin Patch\n{}*** End Patch",
        "*** Delete File: a\n".repeat(count)
    )
}

fn chunks(count: usize) -> String {
    format!(
        "*** Begin Patch\n*** Update File: a\n{}*** End Patch",
        "@@\n+x\n".repeat(count)
    )
}

#[test]
fn files_chunks_and_patches_are_aggregate_inclusive_counts() -> Result<(), ActivityError> {
    for (accepted, rejected) in [
        (
            format!(
                "{}{}",
                template_call(&deletes(16)),
                template_call(&deletes(16))
            ),
            format!(
                "{}{}",
                template_call(&deletes(16)),
                template_call(&deletes(17))
            ),
        ),
        (
            format!(
                "{}{}",
                template_call(&chunks(64)),
                template_call(&chunks(64))
            ),
            format!(
                "{}{}",
                template_call(&chunks(64)),
                template_call(&chunks(65))
            ),
        ),
        (
            template_call(&deletes(1)).repeat(8),
            template_call(&deletes(1)).repeat(9),
        ),
    ] {
        assert_eq!(
            describe(&accepted)?.disposition(),
            SourceDescriptionDisposition::CompleteSyntax
        );
        assert_limit(&describe(&rejected)?);
    }
    Ok(())
}

#[test]
fn retained_charge_is_reserved_before_native_parsing_at_the_exact_limit()
-> Result<(), ActivityError> {
    // Contract charge: 3 framing/header lines, 3,800 content lines, one file,
    // no chunks, 512 patch overhead, and one legacy invocation row (128).
    let mut payload = format!(
        "*** Begin Patch\n*** Add File: a\n{}*** End Patch",
        "+\n".repeat(3_800)
    );
    let fixed_charge = 256 * (3_800 + 4) + 512 + 128;
    let extra_bytes = (1_048_576 - fixed_charge - 8 * payload.len()) / 8;
    payload.insert_str(START.len(), &"x".repeat(extra_bytes));
    assert_eq!(8 * payload.len() + fixed_charge, 1_048_576);
    let complete = describe(&template_call(&payload))?;
    assert_eq!(
        complete.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    payload.insert(START.len(), 'x');
    assert_limit(&describe(&template_call(&payload))?);
    Ok(())
}

#[test]
fn physical_line_limit_does_not_bypass_the_stricter_retained_bound() -> Result<(), ActivityError> {
    // The published retained charge prevents a 4,096-line positive case.
    // Both exact physical maximum and one beyond it must erase the parent.
    for content_lines in [4_093, 4_094] {
        let payload = format!(
            "*** Begin Patch\n*** Add File: a\n{}*** End Patch",
            "+\n".repeat(content_lines)
        );
        assert_limit(&describe(&template_call(&payload))?);
    }
    Ok(())
}

#[test]
fn constructor_rejection_and_expired_budget_never_expose_patch_prefixes()
-> Result<(), ActivityError> {
    let source = template_call(&deletes(1));
    let rejected = CompleteSourceDescriptionInput::from_source(
        "authored-constructor-limit".into(),
        Language::TypeScript,
        &source,
        AnalysisLimits::new(source.len() - 1, Duration::from_secs(1)),
    );
    assert!(matches!(rejected, Err(ActivityError::SourceLimit)));
    let input = CompleteSourceDescriptionInput::from_source(
        "authored-expired-patch".into(),
        Language::TypeScript,
        &source,
        AnalysisLimits::new(32_768, Duration::ZERO),
    )?;
    let description = Analyzer::new()?.describe_complete(input);
    assert_eq!(
        description.disposition(),
        SourceDescriptionDisposition::Fallback(SourceDescriptionFallback::TimeBudget)
    );
    assert!(description.sources().is_empty());
    assert!(description.source_texts().is_empty());
    assert!(description.references().is_empty());
    assert!(description.patches().is_empty());
    Ok(())
}
