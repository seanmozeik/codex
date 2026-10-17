use super::*;
use crate::AnalysisLimits;
use crate::CallId;
use crate::source_description::SourceDescriptionDisposition;
use crate::source_description::SourceReferenceBasis;
use crate::source_description::SourceReferenceKind;
use std::time::Duration;

fn span(source_id: SourceId, end: usize) -> Span {
    Span {
        source_id,
        start_byte: 0,
        end_byte: end,
        start_line: 1,
        end_line: 1,
        start_column: 1,
    }
}

fn reference(source_id: SourceId) -> SourceReference {
    SourceReference {
        basis: SourceReferenceBasis::SourceSyntax,
        kind: SourceReferenceKind::InvocationCall,
        evidence: span(source_id, 1),
        literal: None,
    }
}

#[test]
fn owned_source_entries_do_not_reset_the_shared_extension_charge() {
    let mut collection = Collection::default();
    let root = collection
        .source(Language::TypeScript, "x", None)
        .expect("root source");
    collection.extension(2048, 3).expect("first plan charge");
    collection
        .references(root, vec![reference(root)])
        .expect("root reference");
    let child = collection
        .source(Language::Shell, "x", Some(span(root, 1)))
        .expect("owned child source");
    collection
        .extension(2048, 5)
        .expect("second plan reaches event ceiling");
    collection
        .references(child, vec![reference(child)])
        .expect("child reference");
    assert_eq!(collection.extension_events, MAX_EXTENSION_EVENTS);
    assert_eq!(collection.retained_bytes, 4096 * 128 + 8 + 2 * 128);
    collection
        .source(Language::Python, "x", Some(span(child, 1)))
        .expect("a later source does not create an independent allowance");
    assert_eq!(
        collection.extension(1, 0),
        Err(SourceDescriptionFallback::SourceLimit)
    );
}

#[test]
fn legacy_zero_charge_preserves_the_existing_reference_reservation() {
    let mut collection = Collection::default();
    let root = collection
        .source(Language::Shell, "x", None)
        .expect("root source");
    collection
        .references(root, vec![reference(root)])
        .expect("legacy reference");
    collection
        .extension(0, 0)
        .expect("legacy plan adds no charge");
    let child = collection
        .source(Language::TypeScript, "x", Some(span(root, 1)))
        .expect("legacy child");
    collection
        .extension(0, 0)
        .expect("legacy child adds no charge");
    collection
        .references(child, vec![reference(child)])
        .expect("child reference");
    assert_eq!(collection.extension_events, 0);
    assert_eq!(collection.retained_bytes, 256);
}

#[test]
fn extension_reference_and_patch_charges_share_one_retained_ceiling() {
    let mut collection = Collection::default();
    let root = collection
        .source(Language::TypeScript, "x", None)
        .expect("root source");
    collection
        .references(root, vec![reference(root)])
        .expect("invocation reference");
    collection
        .patch(
            root,
            patch::LiteralPatchInvocation {
                payload: "*** Begin Patch\n*** Delete File: a\n*** End Patch".to_owned(),
                invocation: span(root, 1),
                argument: span(root, 1),
            },
            DescriptionBudget::new(Duration::from_secs(1)),
        )
        .expect("valid submitted patch");
    let prior = collection.retained_bytes;
    assert!(prior > 128);
    // This isolated private-unit boundary is masked by lower source/name
    // limits in many real scripts; it checks the shared logical reservation.
    collection
        .extension(1, MAX_RETAINED_BYTES - prior - 128)
        .expect("extension charge exactly reaches the shared ceiling");
    assert_eq!(collection.retained_bytes, MAX_RETAINED_BYTES);
    assert_eq!(collection.patches.len(), 1);
    assert_eq!(
        collection.references(root, vec![reference(root)]),
        Err(SourceDescriptionFallback::SourceLimit)
    );
}

#[test]
fn extension_arithmetic_rejects_overflow_instead_of_wrapping() {
    let mut events = Collection::default();
    events.extension(1, 0).expect("initial event");
    assert_eq!(
        events.extension(usize::MAX, 0),
        Err(SourceDescriptionFallback::SourceLimit)
    );
    assert_eq!(
        Collection::default().extension(1, usize::MAX),
        Err(SourceDescriptionFallback::SourceLimit)
    );
    let mut retained = Collection::default();
    retained.retain(1).expect("initial retained byte");
    assert_eq!(
        retained.extension(0, usize::MAX),
        Err(SourceDescriptionFallback::SourceLimit)
    );
}

#[test]
fn a_later_owned_child_limit_erases_an_earlier_patch_and_child() {
    let child = "text([0,0,0,0,0,0,0,0,0]);";
    let command =
        serde_json::to_string(&format!("node -e '{child}'")).expect("quote inert command source");
    let patch = serde_json::to_string("*** Begin Patch\n*** Delete File: a\n*** End Patch")
        .expect("quote inert patch source");
    let source = format!(
        "await tools.apply_patch({patch}); await tools.exec_command({{cmd:'cat a'}}); await tools.exec_command({{cmd:{command}}});"
    );
    let input = CompleteSourceDescriptionInput::from_source(
        CallId::from("authored-later-child-limit"),
        Language::TypeScript,
        &source,
        AnalysisLimits::new(32_768, Duration::from_secs(1)),
    )
    .expect("bounded complete input");
    let result = Analyzer::new()
        .expect("pinned parsers")
        .describe_complete(input);
    assert_eq!(
        result.disposition(),
        SourceDescriptionDisposition::Fallback(SourceDescriptionFallback::SourceLimit,)
    );
    assert!(result.sources().is_empty());
    assert!(result.source_texts().is_empty());
    assert!(result.references().is_empty());
    assert!(result.patches().is_empty());
}
