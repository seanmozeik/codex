//! Authored development cases; all scripts are parsed as inert source.

#[path = "source_description_orchestration_extensions/carrier_depth.rs"]
mod carrier_depth;

use codex_code_activity::ActivityError;
use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::CompleteSourceDescriptionInput;
use codex_code_activity::Language;
use codex_code_activity::SourceDescription;
use codex_code_activity::SourceDescriptionDisposition;
use codex_code_activity::SourceDescriptionFallback;
use codex_code_activity::SourceReferenceBasis;
use codex_code_activity::SourceReferenceKind;
use std::time::Duration;

const PATCH_CALL: &str =
    "tools.apply_patch('*** Begin Patch\\n*** Delete File: a\\n*** End Patch')";

fn describe(source: &str) -> Result<SourceDescription, ActivityError> {
    let input = CompleteSourceDescriptionInput::from_source(
        "authored-coverage80-envelope".into(),
        Language::TypeScript,
        source,
        AnalysisLimits::new(32_768, Duration::from_secs(1)),
    )?;
    Ok(Analyzer::new()?.describe_complete(input))
}

fn assert_complete(description: &SourceDescription, source: &str) {
    assert_eq!(
        description.disposition(),
        SourceDescriptionDisposition::CompleteSyntax,
        "{source}"
    );
    assert_eq!(
        description.sources().len(),
        description.source_texts().len()
    );
    for reference in description.references() {
        assert_eq!(reference.basis, SourceReferenceBasis::SourceSyntax);
        let text = &description.source_texts()[reference.evidence.source_id.index()];
        assert!(
            text.get(reference.evidence.start_byte..reference.evidence.end_byte)
                .is_some()
        );
    }
}

fn assert_unsupported(description: &SourceDescription, source: &str) {
    assert_eq!(
        description.disposition(),
        SourceDescriptionDisposition::Fallback(SourceDescriptionFallback::UnsupportedSource,),
        "{source}"
    );
    assert!(description.references().is_empty(), "{source}");
    assert!(description.sources().is_empty(), "{source}");
    assert!(description.source_texts().is_empty(), "{source}");
    assert!(description.patches().is_empty(), "{source}");
}

#[test]
fn poll_rows_keep_exact_spans_and_canonical_option_values() -> Result<(), ActivityError> {
    let source = "/*é*/\ntext(await tools.write_stdin({chars:``,session_id:- /*id*/ 2147483648,yield_time_ms:0,max_output_tokens:9007199254740991}));";
    let description = describe(source)?;
    assert_complete(&description, source);
    assert_eq!(description.source_texts(), [source.to_owned()]);
    let rows: Vec<_> = description
        .references()
        .iter()
        .map(|reference| {
            (
                reference.kind,
                reference.literal.as_deref(),
                source.get(reference.evidence.start_byte..reference.evidence.end_byte),
            )
        })
        .collect();
    assert_eq!(
        rows,
        vec![
            (
                SourceReferenceKind::InvocationCall,
                None,
                Some(
                    "tools.write_stdin({chars:``,session_id:- /*id*/ 2147483648,yield_time_ms:0,max_output_tokens:9007199254740991})"
                )
            ),
            (
                SourceReferenceKind::CommandOption,
                Some(""),
                Some("chars:``")
            ),
            (
                SourceReferenceKind::CommandOption,
                Some("-2147483648"),
                Some("session_id:- /*id*/ 2147483648")
            ),
            (
                SourceReferenceKind::CommandOption,
                Some("0"),
                Some("yield_time_ms:0")
            ),
            (
                SourceReferenceKind::CommandOption,
                Some("9007199254740991"),
                Some("max_output_tokens:9007199254740991")
            ),
            (
                SourceReferenceKind::OutputConsumer,
                None,
                Some(
                    "text(await tools.write_stdin({chars:``,session_id:- /*id*/ 2147483648,yield_time_ms:0,max_output_tokens:9007199254740991}))"
                )
            ),
        ]
    );
    assert!(
        description
            .references()
            .iter()
            .all(|reference| reference.evidence.start_line == 2)
    );
    Ok(())
}

#[test]
fn poll_does_not_stop_later_children_or_create_an_owned_source() -> Result<(), ActivityError> {
    let source =
        "await tools.write_stdin({session_id:0}); text(await tools.exec_command({cmd:'cat a'}));";
    let description = describe(source)?;
    assert_complete(&description, source);
    assert_eq!(
        description.source_texts(),
        [source.to_owned(), "cat a".to_owned()]
    );
    assert_eq!(
        description
            .references()
            .iter()
            .filter(|reference| reference.evidence.source_id.index() == 0)
            .count(),
        4
    );
    Ok(())
}

#[test]
fn nested_display_awaits_keep_source_order_and_one_output_consumer() -> Result<(), ActivityError> {
    let source = "text(JSON.stringify({left:(await tools.exec_command({cmd:'cat a'})).output,right:[await tools.write_stdin({session_id:1}),null]},null,2));";
    let description = describe(source)?;
    assert_complete(&description, source);
    let outer: Vec<_> = description
        .references()
        .iter()
        .filter(|reference| reference.evidence.source_id.index() == 0)
        .map(|reference| (reference.kind, reference.literal.as_deref()))
        .collect();
    assert_eq!(
        outer,
        vec![
            (SourceReferenceKind::InvocationCall, None),
            (SourceReferenceKind::InvocationCall, None),
            (SourceReferenceKind::CommandOption, Some("1")),
            (SourceReferenceKind::OutputConsumer, None),
        ]
    );
    Ok(())
}

#[test]
fn per_index_origins_and_callback_unions_preserve_supported_projections()
-> Result<(), ActivityError> {
    let mixed = format!(
        "[tools.exec_command({{cmd:'cat a'}}),{PATCH_CALL},tools.write_stdin({{session_id:1}})]"
    );
    for source in [
        format!("const rs=await Promise.all({mixed}); text([rs[0].output,rs[1],rs[2].session_id]);"),
        format!("const rs=await Promise.allSettled({mixed}); rs.forEach((row,index)=>{{text({{status:row.status,index:index}});text(row.value);}});"),
        "const rs=await Promise.all([]); rs.forEach(row=>text(row));".into(),
        "const rs=await Promise.allSettled([]); for(const row of rs) {}".into(),
        "const rs=await Promise.all([tools.exec_command({cmd:'cat a'}),tools.write_stdin({session_id:1})]); const copy=rs; for(const row of copy) {text(row.output);text(copy);}".into(),
    ] {
        let description = describe(&source)?;
        assert_complete(&description, &source);
    }
    for source in [
        format!("const rs=await Promise.all({mixed}); text(rs[1].output);"),
        format!("const rs=await Promise.all({mixed}); rs.forEach(row=>text(row.output));"),
        format!(
            "const rs=await Promise.allSettled({mixed}); rs.forEach(row=>text(row.value.output));"
        ),
        "const rs=await Promise.allSettled([]); rs.forEach(row=>text(row.status));".into(),
        "const rs=await Promise.all([tools.exec_command({cmd:'cat a'})]); text(rs[1]);".into(),
        "const rs=await Promise.all([tools.exec_command({cmd:'cat a'})]); text(rs[(0)]);".into(),
    ] {
        assert_unsupported(&describe(&source)?, &source);
    }
    Ok(())
}

#[test]
fn lexical_json_shadowing_and_display_origins_do_not_become_command_text()
-> Result<(), ActivityError> {
    for source in [
        "const JSON='cat a'; text(await tools.exec_command({cmd:JSON}));",
        "const rs=await Promise.all([]); rs.forEach(JSON=>text(JSON));",
        "text(JSON.stringify({ok:true}));",
        "const n=-9007199254740991; const s=('text'); const data=[n,s,{}]; text(data);",
        "((await tools.write_stdin({session_id:2147483647,chars:''})));",
    ] {
        assert_complete(&describe(source)?, source);
    }
    for source in [
        "text(JSON.stringify({ok:true})); const JSON='cat a';",
        "const JSON='cat a'; text(JSON.stringify({ok:true}));",
        "const rs=await Promise.all([]); rs.forEach(JSON=>text(JSON.stringify(JSON)));",
        "const command=('cat a'); await tools.exec_command({cmd:command});",
        "const r=await tools.exec_command({cmd:'cat a'}); const out=r.output; await tools.exec_command({cmd:out});",
        "text({a:1,'a':2});",
        "text({'constr\\u0075ctor':1});",
        "text(JSON.stringify({ok:true},null,(2)));",
        "(text(await tools.exec_command({cmd:'cat a'})));",
        "text(await tools.exec_command({cmd:'cat a'}).output);",
    ] {
        assert_unsupported(&describe(source)?, source);
    }
    Ok(())
}

#[test]
fn invalid_poll_or_hidden_callback_await_erases_earlier_patch_and_command()
-> Result<(), ActivityError> {
    let prefix = format!("await {PATCH_CALL}; await tools.exec_command({{cmd:'cat a'}}); ");
    for suffix in [
        "await tools.write_stdin({session_id:1,chars:'x'});",
        "await tools.write_stdin({session_id:-0});",
        "await tools.write_stdin({session_id:2147483648});",
        "await tools.write_stdin({session_id:-2147483649});",
        "await tools.write_stdin({session_id:1,extra:0});",
        "const empty=''; await tools.write_stdin({session_id:1,chars:empty});",
        "const rs=await Promise.all([]); rs.forEach(row=>text({nested:[await tools.write_stdin({session_id:1})]}));",
        "const rs=await Promise.all([]); for(const row of rs) {text(JSON.stringify([await tools.exec_command({cmd:'cat b'})]));}",
        "const rs=await Promise.all([]); rs.forEach(row=>{text(row); unknown();});",
        "const rs=await Promise.all([]); rs.forEach((row,row)=>text(row));",
        "const rs=await Promise.all([]); rs.forEach(row=>text(row)); text(row);",
    ] {
        let source = format!("{prefix}{suffix}");
        assert_unsupported(&describe(&source)?, &source);
    }
    Ok(())
}

#[test]
fn repeated_text_spelling_keeps_only_enclosing_and_top_level_consumer_spans()
-> Result<(), ActivityError> {
    let source = "const values=await Promise.all([tools.write_stdin({session_id:0})]);values.forEach(item=>text(item.output));for(const item of values){text(item);}const item=1;text(item);";
    let description = describe(source)?;
    assert_complete(&description, source);
    assert_eq!(description.source_texts(), [source.to_owned()]);
    assert!(description.patches().is_empty());
    let rows: Vec<_> = description
        .references()
        .iter()
        .map(|reference| {
            let evidence = &reference.evidence;
            (
                reference.kind,
                reference.literal.as_deref(),
                evidence.source_id.index(),
                evidence.start_byte,
                evidence.end_byte,
                evidence.start_line,
                evidence.end_line,
                evidence.start_column,
            )
        })
        .collect();
    assert_eq!(
        rows,
        vec![
            (
                SourceReferenceKind::InvocationCall,
                None,
                0,
                32,
                65,
                1,
                1,
                33
            ),
            (
                SourceReferenceKind::CommandOption,
                Some("0"),
                0,
                51,
                63,
                1,
                1,
                52
            ),
            (
                SourceReferenceKind::OutputConsumer,
                None,
                0,
                68,
                107,
                1,
                1,
                69
            ),
            (
                SourceReferenceKind::OutputConsumer,
                None,
                0,
                108,
                146,
                1,
                1,
                109
            ),
            (
                SourceReferenceKind::OutputConsumer,
                None,
                0,
                159,
                169,
                1,
                1,
                160
            ),
        ]
    );
    Ok(())
}
