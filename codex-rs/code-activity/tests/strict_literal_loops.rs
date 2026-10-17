//! Authored, inert Python fixtures for the strict sequential-loop boundary.
use codex_code_activity::ActivityError;
use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::CompleteScript;
use codex_code_activity::Effect;
use codex_code_activity::EnvironmentAssumptions;
use codex_code_activity::FileSnapshot;
use codex_code_activity::Language;
use codex_code_activity::PresentationDisposition;
use codex_code_activity::Request;
use codex_code_activity::Target;
use codex_code_activity::WholeScriptActivity;
use pretty_assertions::assert_eq;
use std::time::Duration;

fn strict(source: &str) -> Result<WholeScriptActivity, ActivityError> {
    let request = Request::new(
        "loop-development".into(),
        Language::Python,
        source.into(),
        None,
    )
    .with_snapshots(vec![FileSnapshot::new(
        "a".into(),
        Some("old".into()),
        Some("new".into()),
    )]);
    let script = CompleteScript::from_request(
        request,
        EnvironmentAssumptions::FreshProcess,
        AnalysisLimits::new(32 * 1024, Duration::from_secs(5)),
    )?;
    Ok(Analyzer::new()?.analyze_complete(script))
}

fn effects(activity: &WholeScriptActivity) -> Vec<String> {
    activity
        .report()
        .operations
        .iter()
        .map(|operation| {
            let (name, target) = match &operation.effect {
                Effect::FileRead { target } => ("read", target),
                Effect::FileWrite { target, .. } => ("write", target),
                Effect::FileEdit { target, .. } => ("edit", target),
                Effect::FileOpen { target, .. } => ("open", target),
                other => return format!("unexpected:{other:?}"),
            };
            match target {
                Target::Literal { path, .. } => format!("{name}:{path}"),
                Target::Unresolved => format!("{name}:unresolved"),
                other => format!("{name}:{other:?}"),
            }
        })
        .collect()
}

fn eligible(source: &str, expected: &[&str]) -> Result<(), ActivityError> {
    let activity = strict(source)?;
    assert_eq!(
        activity.disposition(),
        PresentationDisposition::Eligible,
        "{source}"
    );
    assert_eq!(effects(&activity), expected, "{source}");
    Ok(())
}

fn fallback(source: &str) -> Result<(), ActivityError> {
    let activity = strict(source)?;
    assert!(
        matches!(activity.disposition(), PresentationDisposition::Fallback(_)),
        "{source}"
    );
    assert!(activity.report().operations.is_empty(), "{source}");
    assert!(activity.ui_rows().is_empty(), "{source}");
    assert!(activity.diffs().is_empty(), "{source}");
    Ok(())
}

#[test]
fn reads_lists_tuples_and_repeated_paths_in_order() -> Result<(), ActivityError> {
    for sequence in ["['a','b']", "('a','b')"] {
        eligible(
            &format!("for p in {sequence}:\n open(p).read()"),
            &["read:a", "read:b"],
        )?;
    }
    eligible(
        "for p in ['a','a']:\n open(p).read()",
        &["read:a", "read:a"],
    )?;
    eligible(
        "for p in ['a', # comment\n 'b']:\n open(p).read()",
        &["read:a", "read:b"],
    )
}

#[test]
fn multi_file_writes_and_edits_remain_distinct() -> Result<(), ActivityError> {
    eligible(
        "for p in ['a','b']:\n open(p,'w').write('new')",
        &["open:a", "write:a", "open:b", "write:b"],
    )?;
    eligible(
        "for p in ['a','b']:\n s=open(p).read()\n open(p,'w').write(s.replace('old','new'))",
        &["read:a", "open:a", "edit:a", "read:b", "open:b", "edit:b"],
    )
}

#[test]
fn resolved_aliases_handles_and_literal_path_folds_compose() -> Result<(), ActivityError> {
    eligible(
        "reader=open\nfor p in ['a','b']:\n h=reader(p)\n h.read()",
        &["read:a", "read:b"],
    )?;
    eligible(
        "for p in ['a.old','b.old']:\n open(p.replace('.old','.new'),'w').write('x')",
        &["open:a.new", "write:a.new", "open:b.new", "write:b.new"],
    )?;
    eligible(
        "for p in ['a','b']:\n save=open(p+'.txt','w').write\n save('x')",
        &["open:a.txt", "write:a.txt", "open:b.txt", "write:b.txt"],
    )
}

#[test]
fn final_binding_is_sequential_and_body_rebinding_wins() -> Result<(), ActivityError> {
    eligible(
        "p='before'\nfor p in ['a','b']:\n open(p).read()\nopen(p).read()",
        &["read:a", "read:b", "read:b"],
    )?;
    eligible(
        "for p in ['a','b']:\n open(p).read()\n p='after'\nopen(p).read()",
        &["read:a", "read:b", "read:after"],
    )?;
    eligible(
        "last='before'\nfor p in ['a','b']:\n last=p\nopen(last).read()",
        &["read:b"],
    )
}

#[test]
fn rejects_empty_dynamic_non_text_and_oversized_sequences() -> Result<(), ActivityError> {
    for source in [
        "for p in []:\n open('a','w').write('x')",
        "p='before'\nfor p in ():\n unknown()\nopen(p).read()",
        "paths=['a']\nfor p in paths:\n open(p).read()",
        "for p in ['a',1]:\n open(p).read()",
        "for p in [b'a']:\n open(p).read()",
        "for p in [f'a']:\n open(p).read()",
        "for p in ['a'+'b']:\n open(p).read()",
        "for p in [*['a']]:\n open(p).read()",
        "for p,q in [('a','b')]:\n open(p).read()",
    ] {
        fallback(source)?;
    }
    let sixteen = std::iter::repeat_n("'a'", 16).collect::<Vec<_>>().join(",");
    eligible(
        &format!("for p in [{sixteen}]:\n open(p).read()"),
        &["read:a"; 16],
    )?;
    fallback(&format!("for p in [{sixteen},'b']:\n open(p).read()"))
}

#[test]
fn rejects_unsupported_body_shapes_before_any_iteration() -> Result<(), ActivityError> {
    for body in [
        "if True:\n  open(p).read()",
        "for q in ['b']:\n  open(q).read()",
        "break",
        "continue",
        "return p",
        "yield p",
        "await p",
        "raise Exception()",
        "import os",
        "from pathlib import Path",
        "def f():\n  open(p).read()",
        "class C:\n  pass",
        "f=lambda: open(p).read()",
        "try:\n  open(p).read()\n except:\n  pass",
        "with open(p) as h:\n  h.read()",
        "global p",
        "nonlocal p",
        "p += 'x'",
        "(q := p)",
        "p: str = 'x'",
        "p=q='x'",
        "p[0]='x'",
        "p.attr='x'",
        "flag and open(p).read()",
        "[open(q).read() for q in ['a']]",
        "open(p, 'w', 'extra')",
        "open(p,'w',mode='r')",
        "open(p, opener=print)",
    ] {
        fallback(&format!("for p in ['a']:\n {body}"))?;
    }
    fallback("for p in ['a']:\n open(p).read()\nelse:\n open('b').read()")
}

#[test]
fn typed_call_gate_rejects_functions_callbacks_and_generic_data_members()
-> Result<(), ActivityError> {
    for source in [
        "def read(p):\n return open(p).read()\nfor p in ['a']:\n read(p)",
        "f=lambda p: open(p).read()\nfor p in ['a']:\n f(p)",
        "s=1\nfor p in ['a']:\n s.replace('a','b')\n open(p).read()",
        "s={}\nfor p in ['a']:\n s.replace('a','b')\n open(p).read()",
        "def cb():\n return 'x'\nfor p in ['a']:\n open(p,'w').write(cb)",
        "for p in ['a']:\n open(p).read().replace('a', unknown)",
        "for p in ['a']:\n open(p,'w').write(unknown)",
    ] {
        fallback(source)?;
    }
    Ok(())
}

#[test]
fn late_rebinding_and_surrounding_gaps_discard_the_whole_report() -> Result<(), ActivityError> {
    for source in [
        "reader=open\nfor p in ['a','b']:\n reader(p).read()\n reader='no'",
        "for p in ['a','b']:\n open(p).read()\n open='no'",
        "unknown()\nfor p in ['a']:\n open(p).read()",
        "for p in ['a']:\n open(p).read()\nunknown()",
        "for p in ['a']:\n open(p).read()\n unknown()",
        "for p in ['a']:\n open(p).read()\n(",
        "import pathlib\nfor p in ['a']:\n open(p).read()",
    ] {
        fallback(source)?;
    }
    Ok(())
}

#[test]
fn baseline_keeps_conditional_loop_diagnostics_and_binding_join() -> Result<(), ActivityError> {
    let report = Analyzer::new()?.analyze(Request::new(
        "baseline".into(),
        Language::Python,
        "p='before'\nfor p in ['a','b']:\n open(p).read()\nopen(p).read()".into(),
        None,
    ));
    assert!(
        report
            .unresolved
            .iter()
            .any(|gap| gap.reason == "Loop effects are conditional on runtime iteration")
    );
    assert_eq!(report.operations.len(), 3);
    assert!(matches!(
        &report.operations[2].effect,
        Effect::FileRead {
            target: Target::Unresolved
        }
    ));
    Ok(())
}

#[test]
fn unused_unproven_operators_and_members_still_invalidate_loops() -> Result<(), ActivityError> {
    for expression in [
        "unknown+unknown",
        "open(p).read()+1",
        "1+1",
        "unknown.attr",
        "open.missing",
        "open(p).read().unknown",
        "1 .replace",
        "obj.attr",
        "f.attr",
    ] {
        fallback(&format!("for p in ['a']:\n {expression}\n open(p).read()"))?;
    }
    Ok(())
}

#[test]
fn unused_unresolved_identifiers_clear_prefix_effects_and_snapshots() -> Result<(), ActivityError> {
    for body in ["missing", "x=missing"] {
        fallback(&format!(
            "open('prefix','w').write('before')\nfor p in ['a']:\n open(p).read()\n {body}\n open(p,'w').write('after')"
        ))?;
    }
    eligible(
        "for p in ['a']:\n h=open(p, mode='r')\n read=h.read\n read()",
        &["read:a"],
    )
}
