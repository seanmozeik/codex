//! Inline filesystem receivers and closed computed names remain source syntax.

use codex_code_activity::ActivityError;
use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::CompleteScript;
use codex_code_activity::CompleteSourceDescriptionInput;
use codex_code_activity::EnvironmentAssumptions;
use codex_code_activity::Language;
use codex_code_activity::PresentationDisposition;
use codex_code_activity::Request;
use codex_code_activity::SourceDescription;
use codex_code_activity::SourceDescriptionDisposition;
use codex_code_activity::SourceDescriptionFallback;
use codex_code_activity::SourceReferenceBasis;
use codex_code_activity::SourceReferenceKind;
use pretty_assertions::assert_eq;

fn describe(source: &str) -> Result<SourceDescription, ActivityError> {
    let input = CompleteSourceDescriptionInput::from_source(
        "authored-js-expansion".into(),
        Language::TypeScript,
        source,
        AnalysisLimits::default(),
    )?;
    Ok(Analyzer::new()?.describe_complete(input))
}

#[test]
fn inline_require_keeps_import_and_call_evidence_separate() -> Result<(), ActivityError> {
    let source = "require('node:fs').readFileSync('input.txt','utf8');";
    let description = describe(source)?;
    assert_eq!(
        description.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    assert_eq!(
        description
            .references()
            .iter()
            .map(|reference| (
                reference.kind,
                reference.literal.as_deref(),
                reference.basis,
                source.get(reference.evidence.start_byte..reference.evidence.end_byte),
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                SourceReferenceKind::ImportFrom,
                Some("node:fs"),
                SourceReferenceBasis::SourceSyntax,
                Some("require('node:fs')")
            ),
            (
                SourceReferenceKind::ReadCall,
                Some("input.txt"),
                SourceReferenceBasis::SourceSyntax,
                Some("require('node:fs').readFileSync('input.txt','utf8')")
            ),
        ]
    );
    Ok(())
}

#[test]
fn omitted_write_encoding_accepts_only_closed_text_data() -> Result<(), ActivityError> {
    for source in [
        "require('fs').writeFileSync('output.txt','ink');",
        "const fs=require('fs'); fs.writeFileSync('output.txt','ink');",
        "import fs from 'node:fs'; const data=fs.readFileSync('input.txt','utf8'); fs.writeFileSync('output.txt',data.replace('old','new'));",
    ] {
        let description = describe(source)?;
        assert_eq!(
            description.disposition(),
            SourceDescriptionDisposition::CompleteSyntax,
            "{source}"
        );
        assert_eq!(
            description
                .references()
                .last()
                .map(|reference| (reference.kind, reference.literal.as_deref())),
            Some((SourceReferenceKind::WriteCall, Some("output.txt")))
        );
    }
    Ok(())
}

#[test]
fn computed_fs_method_uses_only_literal_string_concatenation() -> Result<(), ActivityError> {
    for call in [
        "fs['readFileSync']('input.txt','utf8')",
        "fs['read'+'FileSync']('input.txt','utf8')",
        "fs[('read'+('File'+'Sync'))]('input.txt','utf8')",
        "fs['read' /* literal spelling */ + 'FileSync']('input.txt','utf8')",
        "require('fs')['read'+'FileSync']('input.txt','utf8')",
    ] {
        let source = format!("const fs=require('fs'); {call};");
        let description = describe(&source)?;
        assert_eq!(
            description.disposition(),
            SourceDescriptionDisposition::CompleteSyntax,
            "{source}"
        );
        assert_eq!(
            description.references().last().map(|reference| (
                reference.kind,
                reference.literal.as_deref(),
                source.get(reference.evidence.start_byte..reference.evidence.end_byte),
            )),
            Some((SourceReferenceKind::ReadCall, Some("input.txt"), Some(call)))
        );
    }
    let description = describe("require('node:fs')['write'+'FileSync']('output.txt','ink');")?;
    assert_eq!(
        description.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    assert_eq!(
        description
            .references()
            .last()
            .map(|reference| reference.kind),
        Some(SourceReferenceKind::WriteCall)
    );
    Ok(())
}

#[test]
fn new_receivers_do_not_bypass_whole_source_validation() -> Result<(), ActivityError> {
    let prefix = "const fs=require('fs'); fs.writeFileSync('output.txt','ink');";
    for suffix in [
        "unknown();",
        "require = other;",
        "const require = 'shadow';",
        "function require() {}",
        "fs = other;",
        "fs.writeFileSync = other;",
        "fs['write'+'FileSync'] = other;",
        "require('fs').writeFileSync = other;",
        "const alias = fs;",
        "const callback = fs['read'+'FileSync'];",
        "const escaped = require('fs');",
        "console.log(fs);",
        "Object.defineProperty(fs,'readFileSync',{get(){ return other; }});",
        "fs[key]('input.txt','utf8');",
        "const key='readFileSync'; fs[key]('input.txt','utf8');",
        "fs['read'+key]('input.txt','utf8');",
        "fs[('readFileSync', 'readFileSync')]('input.txt','utf8');",
        "fs['read'+1]('input.txt','utf8');",
        "fs['read'-'FileSync']('input.txt','utf8');",
        "fs[`${'read'}FileSync`]('input.txt','utf8');",
        "fs[{toString(){return 'readFileSync';}}]('input.txt','utf8');",
        "fs['read'+unknown()]('input.txt','utf8');",
        "fs?.['read'+'FileSync']('input.txt','utf8');",
        "fs['read'+'FileSync']?.('input.txt','utf8');",
        "fs['read'+'FileSync'].call(fs,'input.txt','utf8');",
        "require?.('fs').readFileSync('input.txt','utf8');",
        "require('fs')?.readFileSync('input.txt','utf8');",
        "require('other').readFileSync('input.txt','utf8');",
        "require(name).readFileSync('input.txt','utf8');",
        "require('f'+'s').readFileSync('input.txt','utf8');",
        "require('fs','extra').readFileSync('input.txt','utf8');",
        "require(...['fs']).readFileSync('input.txt','utf8');",
        "require('fs').promises.readFileSync('input.txt','utf8');",
        "require('fs').readFileSync('input.txt');",
        "require('fs').writeFileSync('output.txt', missing);",
        "require('fs').writeFileSync('output.txt', 1);",
        "require('fs').writeFileSync('output.txt', {});",
        "require('fs').writeFileSync('output.txt', Buffer.from('ink'));",
        "require('fs').writeFileSync('output.txt', 'ink', undefined);",
        "require('fs').writeFileSync('output.txt', 'ink', 'base64');",
        "require('fs').writeFileSync('output.txt', 'ink', 'utf8', 'extra');",
        "require('fs').writeFileSync('output.txt', ...['ink']);",
        "const value=require('fs').writeFileSync('output.txt','ink');",
        "require('fs').writeFileSync('output.txt', console.log('ink'));",
        "console['log']('ink');",
        "'ink'['replace']('ink','other');",
    ] {
        let source = format!("{prefix}{suffix}");
        let description = describe(&source)?;
        assert_eq!(
            description.disposition(),
            SourceDescriptionDisposition::Fallback(SourceDescriptionFallback::UnsupportedSource),
            "{source}"
        );
        assert!(description.references().is_empty(), "{source}");
        assert!(description.sources().is_empty(), "{source}");
    }
    Ok(())
}

#[test]
fn computed_member_bounds_discard_preceding_references() -> Result<(), ActivityError> {
    let oversized = format!("'{}'+'{}'", "a".repeat(8192), "b".repeat(8193));
    let nested = format!("{}'readFileSync'{}", "(".repeat(70), ")".repeat(70));
    for key in [oversized, nested] {
        let source =
            format!("const fs=require('fs'); fs.writeFileSync('a','x'); fs[{key}]('a','utf8');");
        let description = describe(&source)?;
        assert_eq!(
            description.disposition(),
            SourceDescriptionDisposition::Fallback(SourceDescriptionFallback::SourceLimit)
        );
        assert!(description.references().is_empty());
        assert!(description.sources().is_empty());
    }
    Ok(())
}

#[test]
fn syntax_extension_keeps_unknown_runtime_effects_empty() -> Result<(), ActivityError> {
    let source = "require('fs')['write'+'FileSync']('output.txt','ink');";
    assert_eq!(
        describe(source)?.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    let input = CompleteScript::from_request(
        Request::new(
            "unknown-js".into(),
            Language::TypeScript,
            source.into(),
            None,
        ),
        EnvironmentAssumptions::PersistentOrUnknown,
        AnalysisLimits::default(),
    )?;
    let activity = Analyzer::new()?.analyze_complete(input);
    assert!(matches!(
        activity.disposition(),
        PresentationDisposition::Fallback(_)
    ));
    assert!(activity.ui_rows().is_empty());
    Ok(())
}
