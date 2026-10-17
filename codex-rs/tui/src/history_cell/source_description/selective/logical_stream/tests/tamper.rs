//! Restored DTOs pass the same validation gate as live source descriptions.

use super::fixture;
use super::restore;
use crate::history_cell::SourceDescriptionCell;
use codex_protocol::source_description::SourceDescription;
use serde_json::json;

#[test]
fn logical_metadata_tampering_omits_live_and_restored_cells() {
    for (pointer, replacement) in [
        ("/snapshot/scope", json!("operationPoint")),
        ("/snapshot/logicalStream", json!(null)),
        ("/snapshot/logicalStream/binding/streamId", json!(1)),
        (
            "/snapshot/logicalStream/binding/baseFilesystemEpoch",
            json!(1),
        ),
        ("/snapshot/logicalStream/binding/revision", json!(1)),
        (
            "/snapshot/logicalStream/binding/openedAt/startByte",
            json!(0),
        ),
        ("/snapshot/logicalStream/revisionAfter", json!(2)),
        ("/snapshot/logicalStream/mode", json!("truncateWrite")),
        (
            "/snapshot/logicalStream/acceptedCharactersRequired",
            json!(3),
        ),
        (
            "/snapshot/logicalStream/payloadBytes",
            json!([120, 120, 10]),
        ),
        ("/snapshot/after", json!("wrong bytes")),
        ("/snapshot/before", json!(null)),
    ] {
        let mut value = fixture();
        *value["edits"][0]
            .pointer_mut(pointer)
            .expect("authored field") = replacement;
        let input: SourceDescription = serde_json::from_value(value).expect("typed tampered DTO");
        assert!(
            SourceDescriptionCell::new("logical".to_owned(), input.clone()).is_none(),
            "accepted {pointer}"
        );
        assert!(restore(input).is_empty(), "restored {pointer}");
    }
}

#[test]
fn logical_payload_disclosure_is_checked_before_live_or_restored_rendering() {
    for payload in ["\u{1b}[31m", "sk-abcdefghijklmnopqrstuvwx", "\u{202e}"] {
        let mut value = fixture();
        value["edits"][0]["snapshot"]["logicalStream"]["payloadBytes"] = json!(payload.as_bytes());
        let input: SourceDescription = serde_json::from_value(value).expect("typed payload DTO");
        assert!(SourceDescriptionCell::new("logical".to_owned(), input.clone()).is_none());
        assert!(restore(input).is_empty());
    }
}
