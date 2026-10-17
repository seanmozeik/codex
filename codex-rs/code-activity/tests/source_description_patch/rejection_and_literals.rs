use super::DELETE;
use super::SourceDescriptionDisposition;
use super::TestResult;
use super::assert_empty;
use super::assert_eq;
use super::describe;
use super::invocation;

#[test]
fn unsupported_siblings_in_every_position_erase_all_enrichment() -> TestResult {
    let good = invocation(DELETE)?;
    let invalid = invocation("*** Begin Patch\n*** Delete File: a\n*** End Patch\nunknown")?;
    for bad in [
        invalid,
        "tools.write_stdin({session_id:1,chars:'x'})".into(),
        "tools.exec_command({cmd:'cat $(pwd)'})".into(),
        "unknown()".into(),
    ] {
        for source in [
            format!("await {good}; await {bad};"),
            format!("await {bad}; await {good};"),
            format!("await Promise.all([{good},{bad}]);"),
            format!("await Promise.all([{bad},{good}]);"),
        ] {
            let description = describe(&source)?;
            assert_empty(&description);
            let json = serde_json::to_value(&description)?;
            assert!(json.get("patches").is_none());
            assert!(json.get("sourceTexts").is_none());
        }
    }
    Ok(())
}

#[test]
fn aliases_computation_rebinding_and_hidden_arguments_never_become_patches() -> TestResult {
    let literal = serde_json::to_string(DELETE)?;
    for source in [
        format!("const p={literal}; await tools.apply_patch(p);"),
        format!("await tools.apply_patch({literal}+'');"),
        format!("await tools.apply_patch({literal},unknown());"),
        format!("await tools.apply_patch(...[{literal}]);"),
        format!("await tools['apply_patch']({literal});"),
        format!("await tools?.apply_patch({literal});"),
        format!("await tools.appl\\u0079_patch({literal});"),
        format!("const tools='x'; await tools.apply_patch({literal});"),
        format!("tools.apply_patch=unknown; await tools.apply_patch({literal});"),
        format!("const p=tools.apply_patch; await p({literal});"),
        format!("await tools.apply_patch(String.raw`{DELETE}`);"),
        format!("await tools.apply_patch(`{DELETE}${{unknown()}}`);"),
        format!("const r=await tools.apply_patch({literal}); text(r.output);"),
    ] {
        assert_empty(&describe(&source)?);
    }
    Ok(())
}

#[test]
fn canonical_framing_and_payload_predicates_reject_native_leniencies() -> TestResult {
    let cases = [
        "*** Begin Patch\n*** End Patch",
        " *** Begin Patch\n*** Delete File: a\n*** End Patch",
        "*** Begin Patch\n*** Delete File: a\n*** End Patch ",
        "*** Begin Patch\n*** Delete File: a\n*** End Patch\n\n",
        "*** Begin Patch\n*** Delete File: a\n*** End Patch\n*** End Patch",
        "*** Begin Patch\n*** Environment ID: remote\n*** Delete File: a\n*** End Patch",
        "*** Begin Patch\n*** Add File: a\n*** End Patch",
        "*** Begin Patch\n*** Delete File: a\n+body\n*** End Patch",
        "*** Begin Patch\n*** Update File: a\n*** Move to: b\n*** End Patch",
        "*** Begin Patch\n*** Update File: a\n@@\n*** End Patch",
        "*** Begin Patch\n*** Update File: a\n@@ \n+x\n*** End Patch",
        "*** Begin Patch\n*** Update File: a\n@@ anchor \n+x\n*** End Patch",
        "*** Begin Patch\n*** Update File: a\n+x\n\n*** End Patch",
        "*** Begin Patch\n*** Update File: a\n+x\n*** Move to: b\n*** End Patch",
        "*** Begin Patch\n*** Update File: a\n+x\n*** End of File\n@@\n+y\n*** End Patch",
        "*** Begin Patch\n*** Update File: a\n*** End of File\n*** End Patch",
        "*** Begin Patch\n*** Delete File: \n*** End Patch",
        "*** Begin Patch\n*** Delete File: a\t\n*** End Patch",
        "*** Begin Patch\n*** Delete File: a\u{85}b\n*** End Patch",
        "*** Begin Patch\n*** Delete File: a\u{2028}b\n*** End Patch",
        "*** Begin Patch\n*** Add File: a\n+x\0y\n*** End Patch",
        "*** Begin Patch\n*** Add File: a\n+x\ry\n*** End Patch",
        "*** Begin Patch\n*** Add File: a\n+x\u{feff}y\n*** End Patch",
    ];
    for payload in cases {
        assert_empty(&describe(&format!("await {};", invocation(payload)?))?);
    }
    Ok(())
}

#[test]
fn literal_cooking_preserves_source_only_data_without_evaluating_insertions() -> TestResult {
    let cases = [
        (
            r#""*** Begin Patch\n*** Add File: a\n+\x61\u0062\t\u2028\n*** End Patch""#,
            "*** Begin Patch\n*** Add File: a\n+ab\t\u{2028}\n*** End Patch",
        ),
        (
            "`*** Begin Patch\n*** Add File: a\n+\\`\\${mystery()}\n*** End Patch`",
            "*** Begin Patch\n*** Add File: a\n+`${mystery()}\n*** End Patch",
        ),
        (
            "`*** Begin Patch\n*** Add File: a\n+ab\\\r\ncd\n*** End Patch`",
            "*** Begin Patch\n*** Add File: a\n+abcd\n*** End Patch",
        ),
    ];
    for (literal, payload) in cases {
        let description = describe(&format!("await tools.apply_patch({literal});"))?;
        assert_eq!(
            description.disposition(),
            SourceDescriptionDisposition::CompleteSyntax
        );
        assert_eq!(description.patches()[0].payload, payload);
        assert_eq!(description.sources().len(), 1);
    }
    for payload in [
        "*** Begin Patch\n*** Delete File: a b\n*** End Patch",
        "*** Begin Patch\n*** Delete File: a\u{a0}b\n*** End Patch",
        "*** Begin Patch\n*** Delete File: a\u{200b}b\n*** End Patch",
        "*** Begin Patch\n*** Update File: a\n \n*** End of File\n*** Delete File: b\n*** End Patch",
    ] {
        assert_eq!(
            describe(&format!("await {};", invocation(payload)?))?.disposition(),
            SourceDescriptionDisposition::CompleteSyntax
        );
    }
    for literal in [
        r#""*** Begin Patch\r\n*** Delete File: a\n*** End Patch""#,
        r#""*** Begin Patch\n*** Add File: a\n+\u{61}\n*** End Patch""#,
        r#""*** Begin Patch\n*** Add File: a\n+\uD800\n*** End Patch""#,
        "`*** Begin Patch\r\n*** Delete File: a\r\n*** End Patch`",
    ] {
        assert_empty(&describe(&format!("await tools.apply_patch({literal});"))?);
    }
    Ok(())
}
