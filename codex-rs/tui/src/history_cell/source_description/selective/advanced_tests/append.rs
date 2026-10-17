//! Conditional EOF wording stays identical for live and restored descriptions.

use super::HistoryCell;
use super::SourceDescription;
use super::SourceDescriptionCell;
use super::parity;
use super::raw;
use super::span;
use serde_json::Value;
use serde_json::json;

fn fixture() -> Value {
    let source = "const fs=require('fs'); const fd=fs.openSync('out','a'); fs.writeSync(fd,'x');";
    let opening = span(source, "fs.openSync('out','a')");
    let operation = span(source, "fs.writeSync(fd,'x')");
    let assumptions = [
        "standardNodeCommonJsEvaluation",
        "canonicalNodeFs",
        "noConcurrentMutation",
        "regularDescriptorFiles",
        "completeDescriptorWrites",
    ];
    let mut dependencies = assumptions
        .iter()
        .map(|assumption| json!({"type":"capability","assumption":assumption}))
        .collect::<Vec<_>>();
    dependencies.extend([
        json!({"type":"entryWorkingDirectory","cwd":"/workspace"}),
        json!({"type":"filesystemEpoch","epoch":0}),
        json!({"type":"descriptor","descriptorId":0,"openedAt":opening}),
    ]);
    json!({"disposition":"selectiveFindings","sources":[{"id":0,"language":"typeScript","parent":null,"text":source}],"edits":[{
        "ordinal":0,"basis":"standardApiForecast","evidence":operation,
        "details":{"kind":"javaScript","assumptions":assumptions,"prediction":{
            "ordinal":0,"evidence":operation,"target":{"path":"out","cwd":"/workspace"},
            "content":{"type":"literal","value":"x","evidence":span(source,"'x'")},
            "writeEffect":{"type":"descriptorAppend","descriptorId":0,"openedAt":opening},
            "dependencies":dependencies,"predecessorConditions":[opening]
        }},"predecessorConditions":[],"carrierAssumptions":[],"laterContext":"unobserved"
    }],"barriers":[]})
}

#[test]
fn append_is_conditional_eof_relative_in_live_and_restored_views() {
    parity(
        fixture(),
        &[
            "conditional descriptor 0 UTF-8 append at operation-time EOF",
            "descriptor 0 opened at",
            "Assume each descriptor write completes all requested bytes",
            "Assume descriptors refer to regular files",
            "Conditional forecast; execution and final file state are unobserved.",
            "Requires earlier Node operation to complete normally",
        ],
    );
    let description: SourceDescription = serde_json::from_value(fixture()).expect("append DTO");
    let cell = SourceDescriptionCell::new("append".to_owned(), description).expect("valid cell");
    let compact = cell
        .display_lines(160)
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(compact.contains("conditionally append UTF-8 at operation-time EOF"));
    for unsupported in [
        "at byte",
        "advance cursor",
        "preserve cursor",
        "preserve suffix",
    ] {
        assert!(!raw(&cell).contains(unsupported), "{unsupported}");
        assert!(!compact.contains(unsupported), "{unsupported}");
    }
}

#[test]
fn append_with_missing_completion_capability_is_not_displayed() {
    let mut value = fixture();
    value["edits"][0]["details"]["prediction"]["dependencies"]
        .as_array_mut()
        .expect("dependencies")
        .retain(|entry| entry["assumption"] != "completeDescriptorWrites");
    let description: SourceDescription = serde_json::from_value(value).expect("typed DTO");
    assert!(SourceDescriptionCell::new("append".to_owned(), description).is_none());
}
