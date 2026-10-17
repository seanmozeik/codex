//! Authored development checks for the sealed extension accounting contract.

use super::DescriptionBudget;
use super::Invocation;
use super::Plan;
use super::extract;
use std::time::Duration;
use tree_sitter::Parser;

fn plan(source: &str) -> Plan {
    let budget = DescriptionBudget::new(Duration::from_secs(1));
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        .expect("the pinned TypeScript grammar loads");
    let tree = crate::source_description::parse(&mut parser, source, budget)
        .expect("authored source parses within the inherited limits");
    extract(tree.root_node(), source, budget).expect("authored source matches the sealed grammar")
}

#[test]
fn legacy_text_bindings_and_callback_shapes_add_no_extension_charge() {
    for source in [
        "const JSON='cat a'; const command=JSON; text(await tools.exec_command({cmd:command}));",
        "const rs=await Promise.all([tools.exec_command({cmd:'cat a'})]); rs.forEach(r=>text(r));",
        "const rs=await Promise.all([tools.exec_command({cmd:'cat a'})]); for(const r of rs) text(r);",
        "const rs=await Promise.all([tools.exec_command({cmd:'cat a'})]); for(const r of rs) {text(r);}",
    ] {
        let plan = plan(source);
        assert_eq!(
            (plan.extension_events, plan.extension_name_bytes),
            (0, 0),
            "{source}"
        );
    }
}

#[test]
fn poll_charges_invocation_and_present_pairs_and_has_no_command_child() {
    let plan = plan(
        "text(await tools.write_stdin({session_id:- /*id*/ 1,chars:``,yield_time_ms:0,max_output_tokens:1}));",
    );
    assert_eq!((plan.extension_events, plan.extension_name_bytes), (5, 0));
    assert!(matches!(plan.invocations.as_slice(), [Invocation::Poll(_)]));
    assert_eq!(plan.references.len(), 6);
}

#[test]
fn display_values_charge_occurrences_and_keep_origins_bounded() {
    let plan = plan(
        "const r=await tools.exec_command({cmd:'cat a'}); const alias=r; const data={out:alias.output,ok:true}; text(JSON.stringify([data,(null)],null,2));",
    );
    // alias binding 1; data binding+record+two pairs+member+bool 6;
    // stringify+array+parentheses+null+explicit null+indent 6.
    assert_eq!((plan.extension_events, plan.extension_name_bytes), (13, 0));
    assert_eq!(plan.invocations.len(), 1);
    assert_eq!(plan.references.len(), 2);
}

#[test]
fn primitive_display_bindings_charge_both_the_value_and_binding() {
    let plan = plan("const n=-1; const s=('x'); text([n,s]);");
    assert_eq!((plan.extension_events, plan.extension_name_bytes), (6, 0));
}

#[test]
fn extended_callbacks_charge_blocks_statements_and_local_names_once() {
    let prefix = "const rs=await Promise.all([tools.exec_command({cmd:'cat a'})]); ";
    for (suffix, events, names) in [
        ("rs.forEach((row,index)=>text(row));", 2, 8),
        ("rs.forEach(row=>{text(row);});", 3, 3),
        ("rs.forEach(row=>{text(row.output);text('done');;});", 7, 3),
        (
            "for(const row of rs) {text(row.output);text('done');;}",
            7,
            3,
        ),
        ("rs.forEach(row=>{});", 2, 3),
        ("for(const row of rs) {}", 2, 3),
    ] {
        let plan = plan(&format!("{prefix}{suffix}"));
        assert_eq!(
            (plan.extension_events, plan.extension_name_bytes),
            (events, names),
            "{suffix}"
        );
    }
}
