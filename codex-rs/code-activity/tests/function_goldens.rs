//! Golden programs are inert input strings; this suite executes no submitted code.
use codex_code_activity::ActivityStream;
use codex_code_activity::Analyzer;
use codex_code_activity::Basis;
use codex_code_activity::Language;
use codex_code_activity::Request;
use pretty_assertions::assert_eq;
use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    name: String,
    language: Language,
    source: String,
    effects: Vec<serde_json::Value>,
    gap: Option<String>,
}

#[test]
fn local_functions_preserve_exact_effects_and_source_evidence() -> anyhow::Result<()> {
    let fixtures: Vec<Fixture> = serde_json::from_str(include_str!("fixtures/functions.json"))?;
    let mut analyzer = Analyzer::new()?;
    for fixture in fixtures {
        let report = analyzer.analyze(Request::new(
            "golden".into(),
            fixture.language,
            fixture.source.clone(),
            Some("/workspace".into()),
        ));
        let effects = report
            .operations
            .iter()
            .map(|op| serde_json::to_value(&op.effect))
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(
            effects, fixture.effects,
            "{}: {:?}",
            fixture.name, report.unresolved
        );
        if fixture.gap.is_none() {
            assert!(
                report.unresolved.is_empty(),
                "{}: {:?}",
                fixture.name,
                report.unresolved
            );
        }
        if let Some(gap) = fixture.gap {
            assert!(
                report
                    .unresolved
                    .iter()
                    .any(|item| item.reason.contains(&gap)),
                "{}: {:?}",
                fixture.name,
                report.unresolved
            );
        }
        for op in &report.operations {
            assert_eq!(op.basis, Basis::StaticIntent);
            let source = &report.sources[op.evidence.source_id.index()];
            assert!(op.evidence.start_byte < op.evidence.end_byte);
            assert_eq!(source.id, op.evidence.source_id);
            if source.id.index() == 0 {
                assert!(op.evidence.end_byte <= fixture.source.len());
            } else {
                assert!(source.parent.is_some());
            }
        }
    }
    Ok(())
}

#[test]
fn function_streams_rebuild_the_same_final_report_at_utf8_boundaries() -> anyhow::Result<()> {
    for (language, source) in [
        (
            Language::Python,
            "from pathlib import Path\ndef make(p):\n return lambda: Path(p).unlink()\nf=make('café')\nf()\n",
        ),
        (
            Language::TypeScript,
            "import fs from 'node:fs'; const save=(p:string)=>fs.writeFileSync(p,'新');function run(p:string){save(p)}run('café');",
        ),
        (
            Language::Shell,
            "node -e 'const f=p=>require(\"fs\").unlinkSync(p);f(\"café\")'",
        ),
    ] {
        let expected = Analyzer::new()?.analyze(Request::new(
            "stream".into(),
            language,
            source.into(),
            /*cwd*/ None,
        ));
        let mut stream = ActivityStream::new("stream".into(), language, /*cwd*/ None)?;
        for character in source.chars() {
            stream.append(&character.to_string())?;
        }
        assert_eq!(stream.finish()?.report(), &expected);
    }
    Ok(())
}
