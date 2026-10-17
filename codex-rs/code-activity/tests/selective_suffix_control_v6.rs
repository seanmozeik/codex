//! Authored before implementation. Parser-only tests; never execute source.
use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::CallId;
use codex_code_activity::Language;
use codex_code_activity::SelectiveEditHostProfile;
use codex_code_activity::SelectiveEditInput;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::time::Duration;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn prefix() -> String {
    format!(
        "text(await tools.apply_patch({}));\n",
        json!(
            "*** Begin Patch\n*** Add File: /synthetic/first.txt\n+one\n*** Add File: /synthetic/second.txt\n+two\n*** End Patch\n"
        )
    )
}
fn later() -> String {
    format!(
        "text(await tools.apply_patch({}));",
        json!(
            "*** Begin Patch\n*** Add File: /synthetic/later.txt\n+never forecast\n*** End Patch\n"
        )
    )
}
fn analyze(source: &str, profile: bool) -> TestResult<Value> {
    let mut analyzer = Analyzer::new()?;
    let input = SelectiveEditInput::from_source(
        CallId::from("public-v6"),
        Language::TypeScript,
        source,
        None,
        AnalysisLimits::new(32768, Duration::from_secs(2)),
    )?;
    if profile {
        Ok(
            serde_json::to_value(analyzer.forecast_edits_with_host_profile(
                input,
                SelectiveEditHostProfile::FreshBuiltinCodeModePatchDisplayV1,
            ))?["analysis"]
                .clone(),
        )
    } else {
        Ok(serde_json::to_value(analyzer.forecast_edits(input))?)
    }
}
fn retained(suffix: &str) -> TestResult {
    let report = analyze(&(prefix() + suffix), true)?;
    assert_eq!(report["disposition"], json!("analyzed"));
    let edits = report["edits"].as_array().ok_or("expected edits array")?;
    assert_eq!(edits.len(), 1, "{report}");
    assert_eq!(
        edits[0]["details"]["patch"]["files"]
            .as_array()
            .ok_or("expected patch files array")?
            .len(),
        2
    );
    assert_eq!(edits[0]["laterContext"], json!("opaqueSuffix"));
    Ok(())
}
fn rejected(suffix: &str) -> TestResult {
    let report = analyze(&(prefix() + suffix), true)?;
    assert!(
        report["edits"]
            .as_array()
            .ok_or("expected edits array")?
            .is_empty(),
        "{report}"
    );
    assert_ne!(report["disposition"], json!("analyzed"), "{report}");
    Ok(())
}
#[test]
fn realistic_suffix_retains_only_literal_prefix() -> TestResult {
    retained(
        "const results = await Promise.allSettled([tools.exec_command({cmd:'echo public'}), tools.unknown_method({value:'public'})]); for(let i=0;i<results.length;i++){ const r=results[i]; if(r.status==='fulfilled'){ const value=r.value.content ?? r.value; helper('label'+i,value); text({i,value}); } else text(r); }",
    )?;
    Ok(())
}
#[test]
fn control_and_following_patch_stay_opaque() -> TestResult {
    for control in [
        "for(let i=0;i<items.length;i++){helper(i);}",
        "if(flag){helper(flag);} else helper(0);",
        "{helper(0);}",
    ] {
        let source = prefix() + control + &later();
        retained(&(control.to_owned() + &later()))?;
        let report = analyze(&source, true)?;
        assert!(
            report["barriers"]
                .as_array()
                .expect("barriers array")
                .iter()
                .any(|b| b["reason"] == "unsupportedControl")
        );
    }
    Ok(())
}
#[test]
fn loop_body_patch_is_not_traversed() -> TestResult {
    retained(&format!("for(let i=0;i<items.length;i++){{ {} }}", later()))?;
    Ok(())
}
#[test]
fn block_scopes_allow_shadowing_without_rebinding_host() -> TestResult {
    retained(
        "const i=0; for(let i=0;i<items.length;i++){ const i=1; {const i=2; const tools=helper; const text=helper; text(i);} }",
    )?;
    Ok(())
}
#[test]
fn module_bindings_and_hoists_reject_prefix() -> TestResult {
    for suffix in [
        "const tools=helper;",
        "let text=helper;",
        "var tools;",
        "function text(){}",
        "for(let i=0;i<2;i++){var tools;}",
        "{function text(){}}",
    ] {
        rejected(suffix)?;
    }
    Ok(())
}
#[test]
fn early_errors_and_typescript_reject_whole_source() -> TestResult {
    for suffix in [
        "{let x=0;const x=1;}",
        "for(let eval=0;eval<2;eval++){}",
        "{const arguments=1;}",
        "{const implements=1;}",
        "for(let i:number=0;i<2;i++){}",
        "if(flag){const x=;}",
        "for(let i=0;i<2;i++){",
        "for(let i=0;i<2;i++){const x=1;const x=2;}",
        "if(flag) const x=1;",
        "for(let i=0;i<2;i++) let x=1;",
        "for(let i=0;i<2;eval++){}",
        "for(let i=0;i<2;arguments++){}",
    ] {
        rejected(suffix)?;
    }
    Ok(())
}
#[test]
fn default_mode_admits_control_shape_but_keeps_display_consumer_opaque() -> TestResult {
    for suffix in [
        "for(let i=0;i<2;i++){}",
        "if(flag){text(flag);}",
        "{const x=1;}",
    ] {
        let report = analyze(&(prefix() + suffix + &later()), false)?;
        assert_eq!(report["disposition"], json!("analyzed"));
        assert_eq!(report["edits"].as_array().expect("edits array").len(), 1);
        assert_eq!(report["edits"][0]["laterContext"], json!("opaqueSuffix"));
        assert_eq!(
            report["barriers"].as_array().expect("barriers array").len(),
            1
        );
        assert_eq!(report["barriers"][0]["reason"], json!("unknownExecution"));
    }
    Ok(())
}

#[test]
fn shorthand_suffix_retains_prefix_but_blocks_successors() -> TestResult {
    for suffix in [
        "const value=1; const obj={value};",
        "const value=1; const obj=[{value}];",
        "const value=1; const obj={nested:{value}};",
    ] {
        retained(&(suffix.to_owned() + &later()))?;
    }
    Ok(())
}
#[test]
fn shorthand_command_arguments_stay_opaque() -> TestResult {
    for suffix in [
        "const cmd='echo public'; await tools.exec_command({cmd});",
        "const cmd='echo public'; text(await tools.exec_command({cmd}));",
        "const workdir='/synthetic'; await tools.exec_command({cmd:'echo public',workdir});",
    ] {
        retained(&(suffix.to_owned() + &later()))?;
    }
    Ok(())
}

#[test]
fn computed_call_callees_remain_outside_finite_grammar() -> TestResult {
    // Frozen HP027's grammar exclusion includes peeled/parenthesized callees.
    // This is deliberately a whole-module exclusion, even after a known prefix.
    for callee in [
        "globalThis[\"text\"]",
        "(globalThis[\"text\"])",
        "((globalThis[\"text\"]))",
    ] {
        let source = format!(
            "{callee}(await tools.apply_patch({}));\n{}",
            json!(
                "*** Begin Patch\n*** Add File: /synthetic/computed.txt\n+authored\n*** End Patch\n"
            ),
            later()
        );
        for candidate in [source.clone(), prefix() + &source] {
            for profile in [false, true] {
                let report = analyze(&candidate, profile)?;
                assert_eq!(
                    report["disposition"],
                    json!({"unavailable":"unsupportedSource"}),
                    "{report}"
                );
                assert!(report["edits"].as_array().expect("edits array").is_empty());
                assert!(
                    report["barriers"]
                        .as_array()
                        .expect("barriers array")
                        .is_empty()
                );
                assert!(
                    report["sources"]
                        .as_array()
                        .expect("sources array")
                        .is_empty()
                );
            }
        }
    }
    Ok(())
}
#[test]
fn subscript_value_read_remains_an_opaque_suffix() -> TestResult {
    retained(&("const results=[]; const value=results[0];".to_owned() + &later()))?;
    Ok(())
}
