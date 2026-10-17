//! Submitted patch syntax, exact cooked evidence, and atomic sibling rejection.

#[path = "source_description_patch/rejection_and_literals.rs"]
mod rejection_and_literals;

use codex_code_activity::ActivityError;
use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::CompleteSourceDescriptionInput;
use codex_code_activity::Language;
use codex_code_activity::PatchEvidence;
use codex_code_activity::PatchLinePrefix;
use codex_code_activity::SourceDescription;
use codex_code_activity::SourceDescriptionDisposition;
use codex_code_activity::SourcePatch;
use codex_code_activity::SourcePatchFile;
use codex_code_activity::SourceReferenceKind;
use pretty_assertions::assert_eq;
use std::time::Duration;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const DELETE: &str = "*** Begin Patch\n*** Delete File: café.txt\n*** End Patch";

fn describe(source: &str) -> Result<SourceDescription, ActivityError> {
    let input = CompleteSourceDescriptionInput::from_source(
        "authored-patch".into(),
        Language::TypeScript,
        source,
        AnalysisLimits::new(32_768, Duration::from_secs(1)),
    )?;
    Ok(Analyzer::new()?.describe_complete(input))
}

fn invocation(payload: &str) -> Result<String, serde_json::Error> {
    Ok(format!(
        "tools.apply_patch({})",
        serde_json::to_string(payload)?
    ))
}

fn evidence_text(patch: &SourcePatch, evidence: PatchEvidence) -> Option<&str> {
    assert_eq!(evidence.patch_id, patch.id);
    patch.payload.get(evidence.range.start..evidence.range.end)
}

fn assert_empty(description: &SourceDescription) {
    assert!(matches!(
        description.disposition(),
        SourceDescriptionDisposition::Fallback(_)
    ));
    assert!(description.sources().is_empty());
    assert!(description.source_texts().is_empty());
    assert!(description.references().is_empty());
    assert!(description.patches().is_empty());
}

#[test]
fn all_directives_keep_exact_paths_prefix_order_and_separate_coordinates() -> TestResult {
    let payload = "*** Begin Patch\n*** Add File: café.txt\n+hello\n+\n*** Delete File: old.txt\n*** Update File: src.rs\n*** Move to: lib.rs\n before\n-old\n+new\n@@ fn f\n+first\n-second\n same\n*** End of File\n*** End Patch\n";
    let call = invocation(payload)?;
    let source = format!("/*é*/\ntext(await {call});");
    let description = describe(&source)?;
    assert_eq!(
        description.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    assert_eq!(description.sources().len(), 1);
    assert_eq!(description.source_texts(), std::slice::from_ref(&source));
    assert_eq!(description.patches().len(), 1);
    let patch = &description.patches()[0];
    assert_eq!(patch.id.index(), 0);
    assert_eq!(patch.payload, payload);
    assert_eq!(patch.invocation.source_id.index(), 0);
    assert_eq!(patch.invocation.start_line, 2);
    assert_eq!(
        source.get(patch.invocation.start_byte..patch.invocation.end_byte),
        Some(call.as_str())
    );
    let literal = serde_json::to_string(payload)?;
    assert_eq!(
        source.get(patch.argument.start_byte..patch.argument.end_byte),
        Some(literal.as_str())
    );
    assert_eq!(patch.files.len(), 3);
    let SourcePatchFile::Add { path, lines } = &patch.files[0] else {
        return Err("expected Add directive".into());
    };
    assert_eq!(path.spelling.as_str(), "café.txt");
    assert_eq!(evidence_text(patch, path.evidence), Some("café.txt"));
    assert_eq!(lines.len(), 2);
    assert_eq!(evidence_text(patch, lines[0].evidence), Some("+hello"));
    assert_eq!(evidence_text(patch, lines[0].text), Some("hello"));
    assert_eq!(evidence_text(patch, lines[1].evidence), Some("+"));
    assert_eq!(evidence_text(patch, lines[1].text), Some(""));
    let SourcePatchFile::Delete { path } = &patch.files[1] else {
        return Err("expected Delete directive".into());
    };
    assert_eq!(path.spelling.as_str(), "old.txt");
    assert_eq!(evidence_text(patch, path.evidence), Some("old.txt"));
    let SourcePatchFile::Update {
        path,
        move_to,
        chunks,
    } = &patch.files[2]
    else {
        return Err("expected Update directive".into());
    };
    assert_eq!(evidence_text(patch, path.evidence), Some("src.rs"));
    let destination = move_to.as_ref().ok_or("expected Move directive")?;
    assert_eq!(destination.spelling.as_str(), "lib.rs");
    assert_eq!(evidence_text(patch, destination.evidence), Some("lib.rs"));
    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0].marker, None);
    assert_eq!(chunks[0].end_of_file, None);
    assert_eq!(
        chunks[0]
            .lines
            .iter()
            .map(|line| line.prefix)
            .collect::<Vec<_>>(),
        [
            PatchLinePrefix::Context,
            PatchLinePrefix::Minus,
            PatchLinePrefix::Plus
        ]
    );
    assert_eq!(
        chunks[1]
            .lines
            .iter()
            .map(|line| line.prefix)
            .collect::<Vec<_>>(),
        [
            PatchLinePrefix::Plus,
            PatchLinePrefix::Minus,
            PatchLinePrefix::Context
        ]
    );
    assert_eq!(
        chunks[1]
            .marker
            .and_then(|evidence| evidence_text(patch, evidence)),
        Some("@@ fn f")
    );
    assert_eq!(
        chunks[1]
            .end_of_file
            .and_then(|evidence| evidence_text(patch, evidence)),
        Some("*** End of File")
    );
    assert_eq!(
        description
            .references()
            .iter()
            .map(|reference| reference.kind)
            .collect::<Vec<_>>(),
        [
            SourceReferenceKind::InvocationCall,
            SourceReferenceKind::OutputConsumer
        ]
    );
    assert!(
        description
            .references()
            .iter()
            .all(|reference| reference.literal.is_none())
    );
    let json = serde_json::to_value(&description)?;
    assert!(json.get("sourceTexts").is_none());
    assert!(json["sources"][0].get("text").is_none());
    assert_eq!(json["patches"][0]["files"][2]["kind"], "update");
    assert!(json["patches"][0]["files"][2].get("moveTo").is_some());
    Ok(())
}

#[test]
fn exact_output_consumers_and_trailing_commas_reuse_the_existing_envelope() -> TestResult {
    let call = invocation(DELETE)?;
    let joined = format!("Promise.allSettled([{call},])");
    let sources = [
        format!("await {call};"),
        format!("text(await {call});"),
        format!("const r=await {call}; text(r); text(r);"),
        format!("await {joined};"),
        format!("text(await {joined});"),
        format!("const rs=await {joined}; text(rs);"),
        format!("const rs=await {joined}; rs.forEach(text);"),
        format!("const rs=await {joined}; rs.forEach(r=>text(r));"),
        format!("const rs=await {joined}; rs.forEach((r)=>text(r));"),
        format!("const rs=await {joined}; for(const r of rs) text(r);"),
        format!("const rs=await {joined}; for(const r of rs) {{text(r);}}"),
        format!(
            "/*outer*/await tools /*receiver*/ . apply_patch({},);",
            serde_json::to_string(DELETE)?
        ),
    ];
    for source in sources {
        let description = describe(&source)?;
        assert_eq!(
            description.disposition(),
            SourceDescriptionDisposition::CompleteSyntax,
            "{source}"
        );
        assert_eq!(description.patches().len(), 1);
        assert_eq!(
            description
                .references()
                .iter()
                .filter(|reference| reference.kind == SourceReferenceKind::InvocationCall)
                .count(),
            1
        );
    }
    Ok(())
}

#[test]
fn mixed_calls_preserve_source_ownership_and_reference_order() -> TestResult {
    let call = invocation(DELETE)?;
    let source = format!(
        "const rs=await Promise.all([{call},tools.exec_command({{cmd:'cat a'}}),{call}]); rs.forEach(text);"
    );
    let description = describe(&source)?;
    assert_eq!(
        description.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    assert_eq!(
        description
            .patches()
            .iter()
            .map(|patch| patch.id.index())
            .collect::<Vec<_>>(),
        [0, 1]
    );
    assert_eq!(description.sources().len(), 2);
    assert_eq!(description.source_texts(), [source, "cat a".to_owned()]);
    assert_eq!(
        description
            .references()
            .iter()
            .map(|reference| (
                reference.evidence.source_id.index(),
                reference.kind,
                reference.literal.as_deref()
            ))
            .collect::<Vec<_>>(),
        [
            (0, SourceReferenceKind::InvocationCall, None),
            (0, SourceReferenceKind::InvocationCall, None),
            (0, SourceReferenceKind::InvocationCall, None),
            (0, SourceReferenceKind::OutputConsumer, None),
            (1, SourceReferenceKind::InvocationCall, Some("cat")),
        ]
    );
    for reference in description.references() {
        let text = &description.source_texts()[reference.evidence.source_id.index()];
        assert!(
            text.get(reference.evidence.start_byte..reference.evidence.end_byte)
                .is_some()
        );
    }
    Ok(())
}

#[test]
fn exec_only_serialization_omits_patch_and_private_source_text_fields() -> TestResult {
    let source = "text(await tools.exec_command({cmd:'cat a'}));";
    let description = describe(source)?;
    assert_eq!(
        description.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    assert!(description.patches().is_empty());
    assert_eq!(
        description.sources().len(),
        description.source_texts().len()
    );
    assert_eq!(
        description.source_texts(),
        [source.to_owned(), "cat a".to_owned()]
    );
    let json = serde_json::to_value(&description)?;
    assert!(json.get("patches").is_none());
    assert!(json.get("sourceTexts").is_none());
    assert_eq!(json.as_object().map(serde_json::Map::len), Some(4));
    assert_eq!(json["sources"][1]["language"], "shell");
    assert!(json["sources"][1].get("text").is_none());
    Ok(())
}

#[test]
fn empty_poll_siblings_preserve_submitted_patches_in_every_position() -> TestResult {
    let good = invocation(DELETE)?;
    let poll = "tools.write_stdin({session_id:1,chars:''})";
    for source in [
        format!("await {good}; await {poll};"),
        format!("await {poll}; await {good};"),
        format!("await Promise.all([{good},{poll}]);"),
        format!("await Promise.all([{poll},{good}]);"),
    ] {
        let description = describe(&source)?;
        assert_eq!(
            description.disposition(),
            SourceDescriptionDisposition::CompleteSyntax,
            "{source}"
        );
        assert!(!description.sources().is_empty(), "{source}");
        assert!(!description.references().is_empty(), "{source}");
        assert_eq!(description.source_texts(), std::slice::from_ref(&source));
        assert_eq!(description.patches().len(), 1, "{source}");
        assert_eq!(description.patches()[0].payload, DELETE, "{source}");
    }
    Ok(())
}
