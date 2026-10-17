use crate::source_description::SourceDescription;
use crate::source_description::SourceDescriptionFallback;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

fn fixture() -> Value {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/source_description_selective.json"
    )))
    .expect("authored selective wire fixture")
}

fn description(value: Value) -> SourceDescription {
    serde_json::from_value(value).expect("typed conditional description")
}

fn rejected(value: Value) {
    assert_eq!(
        description(value).into_presentable(|_| true),
        SourceDescription::Fallback {
            reason: SourceDescriptionFallback::Presentation
        },
    );
}

#[test]
fn conditional_patch_and_recipe_round_trip_with_provenance() {
    let value = fixture();
    let input = description(value.clone());
    assert_eq!(input.clone().into_presentable(|_| true), input);
    assert_eq!(serde_json::to_value(input).expect("serialize"), value);
}

#[test]
fn invalid_conditional_evidence_or_order_discards_every_operation() {
    for change in 0..6 {
        let mut value = fixture();
        match change {
            0 => value["edits"][1]["evidence"]["sourceId"] = json!(99),
            1 => value["edits"][1]["ordinal"] = json!(7),
            2 => value["edits"][1]["carrierAssumptions"][0]["invocation"]["sourceId"] = json!(2),
            3 => {
                value["edits"][0]["details"]["patch"]["files"][0]["lines"][0]["prefix"] =
                    json!("minus")
            }
            4 => {
                value["edits"][1]["details"]["prediction"]["dependencies"][1]["evidence"]["sourceId"] =
                    json!(1)
            }
            _ => {
                value["edits"][1]["details"]["prediction"]["operation"]["content"]["input"]["filesystemEpoch"] =
                    json!(1)
            }
        }
        rejected(value);
    }
}

#[test]
fn recipe_read_requires_its_source_local_dependency() {
    let mut value = fixture();
    value["edits"][1]["details"]["prediction"]["dependencies"]
        .as_array_mut()
        .expect("dependencies")
        .retain(|dependency| dependency["type"] != "readValue");
    rejected(value);
}

#[test]
fn local_read_ids_are_not_shared_between_owned_sources() {
    let mut value = fixture();
    let mut source = value["sources"][2].clone();
    source["id"] = json!(3);
    value["sources"]
        .as_array_mut()
        .expect("sources")
        .push(source);
    let mut edit = value["edits"][1].clone();
    edit["ordinal"] = json!(2);
    edit["evidence"]["sourceId"] = json!(3);
    let prediction = &mut edit["details"]["prediction"];
    prediction["evidence"]["sourceId"] = json!(3);
    prediction["operation"]["content"]["evidence"]["sourceId"] = json!(3);
    prediction["operation"]["content"]["input"]["evidence"]["sourceId"] = json!(3);
    prediction["operation"]["content"]["input"]["target"]["path"] = json!("another.txt");
    for dependency in prediction["dependencies"]
        .as_array_mut()
        .expect("dependencies")
    {
        if let Some(evidence) = dependency.get_mut("evidence") {
            evidence["sourceId"] = json!(3);
        }
    }
    prediction["predecessorConditions"][0]["sourceId"] = json!(3);
    value["edits"].as_array_mut().expect("edits").push(edit);
    let input = description(value);
    assert_eq!(input.clone().into_presentable(|_| true), input);
}

#[test]
fn disclosure_checks_target_cwd_and_recipe_strings_without_redaction() {
    for blocked in ["note.txt", "/workspace", "old", "new"] {
        let input = description(fixture());
        assert_eq!(
            input.into_presentable(|text| text != blocked),
            SourceDescription::Fallback {
                reason: SourceDescriptionFallback::Presentation
            },
        );
    }
    let mut value = fixture();
    value["edits"][1]["details"]["prediction"]["operation"]["content"]["new"] = json!("\u{1b}[31m");
    rejected(value);
}

#[test]
fn deep_recipes_and_unaccounted_capabilities_are_rejected() {
    let mut value = fixture();
    let mut recipe = value["edits"][1]["details"]["prediction"]["operation"]["content"].clone();
    let evidence = recipe["evidence"].clone();
    for _ in 0..34 {
        recipe = json!({"type":"replace", "input":recipe, "old":"x", "new":"y", "count":null, "evidence":evidence});
    }
    value["edits"][1]["details"]["prediction"]["operation"]["content"] = recipe;
    rejected(value);
    let mut value = fixture();
    value["edits"][1]["details"]["assumptions"] =
        json!(["standardPythonEvaluation", "noConcurrentMutation"]);
    rejected(value);
}

#[test]
fn conditional_payload_rejects_applied_status_and_unknown_fields() {
    let mut value = fixture();
    value["edits"][0]["applied"] = json!(true);
    assert!(serde_json::from_value::<SourceDescription>(value).is_err());
    let mut value = fixture();
    value["edits"][1]["carrierAssumptions"][0]["basis"] = json!("startupAttested");
    assert!(serde_json::from_value::<SourceDescription>(value).is_err());
}
