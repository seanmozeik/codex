use super::support::TestResult;
use super::support::edits;
use super::support::forecast;
use super::support::prediction;
use pretty_assertions::assert_eq;

#[test]
fn noncanonical_initializers_keep_only_the_preceding_write() -> TestResult {
    let prefix = "const fs=require('fs'); fs.writeFileSync('before','x');";
    for initializer in [
        "helper('fs').writeFileSync",
        "require('fs/promises').writeFileSync",
        "require('node:fs/promises').writeFileSync",
        "require('f'+'s').writeFileSync",
        "require(specifier).writeFileSync",
        "loader.writeFileSync",
        "'fs'.writeFileSync",
        "fs.appendFileSync",
        "fs.promises.writeFile",
        "helper(fs.writeFileSync)",
        "fs.writeFileSync.bind(fs)",
        "fs.writeFileSync.call",
        "'text'.replace",
    ] {
        let source = format!(
            "{prefix} const specifier='fs'; const write={initializer}; write('after','y');"
        );
        let report = forecast(&source)?;
        assert_eq!(edits(&report)?.len(), 1, "{initializer}");
        assert_eq!(prediction(&report, 0)["target"]["path"], "before");
        assert_eq!(report["edits"][0]["laterContext"], "opaqueSuffix");
    }
    Ok(())
}

#[test]
fn escaping_capabilities_and_unmodeled_calls_stop_successor_forecasts() -> TestResult {
    let prefix = "let fs=require('fs'); let write=fs.writeFileSync; write('before','x');";
    for statement in [
        "unknown(fs);",
        "unknown(write);",
        "unknown();",
        "write.call(null,'after','y');",
        "write.apply(null,'after','y');",
        "const result=write('after','y');",
        "let assigned=null; assigned=write;",
        "fs=require('node:fs');",
        "write=fs.writeFileSync;",
        "write=write;",
    ] {
        let report = forecast(&format!("{prefix} {statement} write('after','y');"))?;
        assert_eq!(edits(&report)?.len(), 1, "{statement}");
        assert_eq!(prediction(&report, 0)["target"]["path"], "before");
        assert_eq!(report["edits"][0]["laterContext"], "opaqueSuffix");
        assert_eq!(
            report["javascriptBarriers"].as_array().map(Vec::len),
            Some(1),
            "{statement}"
        );
    }
    Ok(())
}

#[test]
fn shadowing_dynamic_members_mutation_and_destructuring_refuse_whole_source() -> TestResult {
    let prefix = "let fs=require('fs'); const write=fs.writeFileSync; write('before','x');";
    for suffix in [
        "const require='shadow';",
        "const fs='shadow';",
        "const write='shadow';",
        "{ const write=fs.writeFileSync; write('after','y'); }",
        "function write() {}",
        "const {writeFileSync:saved}=fs;",
        "let {writeFileSync:saved}=require('fs');",
        "const {openSync:saved}=require('fs');",
        "const {...saved}=require('fs');",
        "const {writeFileSync:saved=write}=require('fs');",
        "const saved=fs['writeFileSync'];",
        "const key='writeFileSync'; const saved=fs[key];",
        "const saved=fs?.writeFileSync;",
        "write?.('after','y');",
        "fs.writeFileSync=write;",
        "write.extra='x';",
        "const box={write};",
        "const list=[write];",
    ] {
        let report = forecast(&format!("{prefix} {suffix}"))?;
        assert!(edits(&report)?.is_empty(), "{suffix}");
        assert_eq!(
            report["disposition"]["unavailable"], "unsupportedSource",
            "{suffix}"
        );
    }
    Ok(())
}

#[test]
fn capability_aliases_do_not_resolve_lexical_names_before_initialization() -> TestResult {
    for source in [
        "write('out','x'); const write=require('fs').writeFileSync;",
        "const saved=write; const write=require('fs').writeFileSync; saved('out','x');",
        "const write=fs.writeFileSync; let fs=require('fs'); write('out','x');",
    ] {
        let report = forecast(source)?;
        assert!(edits(&report)?.is_empty(), "{source}");
        assert_eq!(report["javascriptBarriers"][0]["reason"], "unknownName");
    }
    Ok(())
}

#[test]
fn new_members_after_object_rebinding_do_not_reuse_earlier_capabilities() -> TestResult {
    let report = forecast(
        "let fs=require('fs'); const write=fs.writeFileSync; fs='changed'; write('before','x'); const later=fs.writeFileSync; later('after','y');",
    )?;
    assert_eq!(edits(&report)?.len(), 1);
    assert_eq!(prediction(&report, 0)["target"]["path"], "before");
    assert_eq!(
        report["javascriptBarriers"][0]["reason"],
        "unsupportedOperation"
    );
    Ok(())
}

#[test]
fn capability_alias_growth_discards_the_prefix_at_the_retained_value_limit() -> TestResult {
    let mut source =
        "const fs=require('fs'); fs.writeFileSync('before','x'); const alias0=fs.writeFileSync;"
            .to_owned();
    for index in 1..160 {
        let previous = index - 1;
        source.push_str(&format!("const alias{index}=alias{previous};"));
    }
    source.push_str("alias159('after','y');");
    let report = forecast(&source)?;
    assert!(edits(&report)?.is_empty());
    assert_eq!(report["disposition"]["unavailable"], "sourceLimit");
    Ok(())
}
