use super::support::Fixture;
use super::support::TestResult;
use super::support::analyze;
use super::support::case;
use super::support::cases;
use super::support::text;
use codex_code_activity::SelectiveEditDisposition;
use codex_code_activity::SnapshotEpoch;
use codex_code_activity::SnapshotPreviewError;
use codex_code_activity::SnapshotPreviewLimit;
use codex_code_activity::SourceDescriptionFallback;
use codex_code_activity::SourceReadId;
use pretty_assertions::assert_eq;

#[test]
fn item_count_and_composed_depth_have_typed_source_limits() -> TestResult {
    let mut count = 0;
    for row in cases()?
        .iter()
        .filter(|row| row["expected"] == "sourceLimit")
    {
        let analysis = analyze(text(row, "source")?)?;
        assert_eq!(
            analysis.disposition(),
            SelectiveEditDisposition::Unavailable(SourceDescriptionFallback::SourceLimit),
            "{}",
            row["id"]
        );
        assert!(analysis.edits().is_empty());
        count += 1;
    }
    assert_eq!(count, 2);
    Ok(())
}

#[test]
fn existing_preview_limits_are_not_bypassed_by_join_lowering() -> TestResult {
    for limit in [
        SnapshotPreviewLimit::Output,
        SnapshotPreviewLimit::Nodes,
        SnapshotPreviewLimit::Depth,
        SnapshotPreviewLimit::Work,
        SnapshotPreviewLimit::Input,
    ] {
        let mut fixture = Fixture::new(&case("J01")?)?;
        match limit {
            SnapshotPreviewLimit::Output => fixture.input.limits.output_bytes = 7,
            SnapshotPreviewLimit::Nodes => fixture.input.limits.nodes = 1,
            SnapshotPreviewLimit::Depth => fixture.input.limits.depth = 1,
            SnapshotPreviewLimit::Work => fixture.input.limits.work = 0,
            SnapshotPreviewLimit::Input => fixture.input.limits.input_bytes = 0,
            _ => return Err("unexpected authored limit".into()),
        }
        assert_eq!(
            fixture.preview().err(),
            Some(SnapshotPreviewError::ResourceLimit { limit })
        );
    }
    Ok(())
}

#[test]
fn repeated_separator_growth_hits_output_limit_not_source_or_nodes() -> TestResult {
    let row = case("J38")?;
    let mut fixture = Fixture::new(&row)?;
    assert_eq!(
        fixture.analysis.disposition(),
        SelectiveEditDisposition::Analyzed
    );
    assert!(fixture.analysis.python_barriers().is_empty());
    assert_eq!(fixture.analysis.edits().len(), 1);
    fixture.input.limits.output_bytes = usize::try_from(
        row["outputBytes"]
            .as_u64()
            .ok_or("authored output ceiling")?,
    )?;
    assert_eq!(
        fixture.preview().err(),
        Some(SnapshotPreviewError::ResourceLimit {
            limit: SnapshotPreviewLimit::Output
        })
    );
    fixture.input.limits.output_bytes = 1_048_576;
    let bytes = fixture.preview()?.after_bytes;
    assert_eq!(bytes.len(), 129_024);
    assert!(bytes.iter().all(|byte| *byte == b's'));
    Ok(())
}

#[test]
fn read_identity_and_epoch_cannot_be_recovered_from_correct_bytes() -> TestResult {
    let row = case("J12")?;
    let mut wrong_id = Fixture::new(&row)?;
    let binding = &mut wrong_id.input.reads[0].binding;
    let expected = binding.read;
    binding.read = SourceReadId::new(expected.source_id, 1);
    assert_eq!(
        wrong_id.preview().err(),
        Some(SnapshotPreviewError::MissingRead { read: expected })
    );
    let mut wrong_epoch = Fixture::new(&row)?;
    wrong_epoch.input.reads[0].binding.filesystem_epoch = SnapshotEpoch::new(1);
    assert_eq!(
        wrong_epoch.preview().err(),
        Some(SnapshotPreviewError::ReadBindingMismatch { read: expected })
    );
    Ok(())
}
