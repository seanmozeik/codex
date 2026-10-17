//! Complete source-reference syntax and atomic fallback regressions.

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
use pretty_assertions::assert_eq;

fn describe(source: &str) -> Result<SourceDescription, ActivityError> {
    let input = CompleteSourceDescriptionInput::from_source(
        "authored-javascript-source".into(),
        Language::TypeScript,
        source,
        AnalysisLimits::default(),
    )?;
    Ok(Analyzer::new()?.describe_complete(input))
}

#[test]
fn file_api_names_and_transform_chain_are_only_source_references() -> Result<(), ActivityError> {
    let source = "const fs = require('node:fs'); const path = 'a/./b'; let text = fs.readFileSync(path, 'utf8'); text = text.replace('old', 'new').replaceAll('x', '$&'); fs.writeFileSync(path, text, 'utf8'); console.log(text);";
    let description = describe(source)?;
    assert_eq!(
        description.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    assert_eq!(
        description
            .references()
            .iter()
            .map(|reference| (reference.kind, reference.literal.as_deref(),))
            .collect::<Vec<_>>(),
        vec![
            (SourceReferenceKind::ImportFrom, Some("node:fs")),
            (SourceReferenceKind::ReadCall, Some("a/./b")),
            (SourceReferenceKind::ReplaceCall, Some("a/./b")),
            (SourceReferenceKind::ReplaceCall, Some("a/./b")),
            (SourceReferenceKind::WriteCall, Some("a/./b")),
            (SourceReferenceKind::OutputConsumer, None),
        ]
    );
    for reference in description.references() {
        assert_eq!(reference.basis, SourceReferenceBasis::SourceSyntax);
        assert!(
            source
                .get(reference.evidence.start_byte..reference.evidence.end_byte)
                .is_some()
        );
    }
    Ok(())
}

#[test]
fn module_forms_and_owned_text_aliases_preserve_literal_paths() -> Result<(), ActivityError> {
    for import in [
        "const fs = require('fs');",
        "import fs from 'node:fs';",
        "import * as fs from 'fs';",
    ] {
        let source = format!(
            "{import} const input = 'caf\\u00e9.txt'; const text = fs.readFileSync(input, 'utf8'); const alias = text; fs.writeFileSync('other.txt', alias, 'utf8');"
        );
        let description = describe(&source)?;
        assert_eq!(
            description.disposition(),
            SourceDescriptionDisposition::CompleteSyntax,
            "{source}"
        );
        assert_eq!(
            description
                .references()
                .iter()
                .filter(|reference| matches!(
                    reference.kind,
                    SourceReferenceKind::ReadCall | SourceReferenceKind::WriteCall
                ))
                .map(|reference| (reference.kind, reference.literal.as_deref()))
                .collect::<Vec<_>>(),
            vec![
                (SourceReferenceKind::ReadCall, Some("café.txt")),
                (SourceReferenceKind::WriteCall, Some("other.txt")),
            ]
        );
    }
    Ok(())
}

#[test]
fn local_mutable_data_assignment_updates_source_origin() -> Result<(), ActivityError> {
    let description = describe(
        "const fs = require('fs'); let p = 'first'; p = 'second'; fs.writeFileSync(p, 'x'.replace('x', 'y'), 'utf8'); print('queued');",
    )?;
    assert_eq!(
        description.disposition(),
        SourceDescriptionDisposition::CompleteSyntax
    );
    assert_eq!(
        description
            .references()
            .iter()
            .map(|reference| (reference.kind, reference.literal.as_deref(),))
            .collect::<Vec<_>>(),
        vec![
            (SourceReferenceKind::ImportFrom, Some("fs")),
            (SourceReferenceKind::ReplaceCall, None),
            (SourceReferenceKind::WriteCall, Some("second")),
            (SourceReferenceKind::OutputConsumer, None),
        ]
    );
    Ok(())
}

#[test]
fn every_unsupported_suffix_discards_prefix_references() -> Result<(), ActivityError> {
    let prefix = "const fs = require('fs'); let text = fs.readFileSync('a', 'utf8');";
    for suffix in [
        "unknown();",
        "missing;",
        "text = missing;",
        "fs = text;",
        "fs.readFileSync = unknown;",
        "fs[missing]('a', 'utf8');",
        "const alias = fs;",
        "const read = fs.readFileSync;",
        "const console = text;",
        "const require = text;",
        "const text = 'shadow';",
        "let text = 'shadow';",
        "const old = 'a'; old = 'b';",
        "globalThis.fs = fs;",
        "Object.setPrototypeOf(text, {});",
        "text.__proto__.replace('a', 'b');",
        "text.replace(/a/g, 'b');",
        "text.replaceAll(/a/g, 'b');",
        "text.replace('a', () => 'b');",
        "text.replace(...text);",
        "text.replace('a', 'b', 1);",
        "text.replace('a');",
        "text.slice(0);",
        "fs.readFileSync('a');",
        "fs.readFileSync('a', 'base64');",
        "fs.readFileSync('a', {encoding: 'utf8'});",
        "fs.readFileSync(text, 'utf8');",
        "fs.readFileSync?.('a', 'utf8');",
        "fs?.readFileSync('a', 'utf8');",
        "fs.readFileSync<string>('a', 'utf8');",
        "fs.writeFileSync('a', text, {encoding: 'utf8'});",
        "fs.writeFileSync('a', 1, 'utf8');",
        "fs.writeFileSync('a', text, 'utf8', 'extra');",
        "fs.writeFileSync('a', Buffer.from(text), 'utf8');",
        "const value = fs.writeFileSync('a', text, 'utf8');",
        "fs.writeFileSync('a', text, 'utf8'); custom();",
        "await fs.writeFileSync('a', text, 'utf8');",
        "if (true) fs.writeFileSync('a', text, 'utf8');",
        "for (const p of ['a']) fs.writeFileSync(p, text, 'utf8');",
        "function helper() { return text; }",
        "class Helper {}",
        "text += 'x';",
        "text = `${text}`;",
        "text = 'a' + 'b';",
        "text = console.log(text);",
        "console['log'](text);",
        "console.log(unknown());",
        "let typed: string = text;",
        "let asserted! = text;",
        "const a = 'x', b = 'y';",
        "const invalid = '\\uD800';",
        "fs.readFileSync('a\\x00b', 'utf8');",
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
fn unsupported_module_forms_and_hoisting_are_not_ignored() -> Result<(), ActivityError> {
    for source in [
        "const fs = require('other'); fs.readFileSync('a', 'utf8');",
        "const fs = require(name); fs.readFileSync('a', 'utf8');",
        "let fs = require('fs'); fs.readFileSync('a', 'utf8');",
        "var fs = require('fs'); fs.readFileSync('a', 'utf8');",
        "const {readFileSync} = require('fs'); readFileSync('a', 'utf8');",
        "import {readFileSync} from 'fs'; readFileSync('a', 'utf8');",
        "import type fs from 'fs'; fs.readFileSync('a', 'utf8');",
        "import fs from 'other'; fs.readFileSync('a', 'utf8');",
        "import 'fs';",
        "export const fs = require('fs');",
        "const data = 'x'; const fs = require('fs'); fs.writeFileSync('a', data, 'utf8');",
        "fs.readFileSync('a', 'utf8'); const fs = require('fs');",
        "const fs = require('fs'); import other from 'fs';",
        "const fs = require?.('fs'); fs.readFileSync('a', 'utf8');",
        "const fs = require('fs'); const \u{212a} = 'a'; fs.readFileSync(\u{212a}, 'utf8');",
        "const fs = require('fs'); fs.readFileSync('a', 'utf8'); //\0",
    ] {
        let description = describe(source)?;
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
fn early_error_binding_names_discard_the_whole_description() -> Result<(), ActivityError> {
    for source in [
        "import arguments from 'fs'; arguments.readFileSync('a','utf8');",
        "import * as await from 'fs'; await.readFileSync('a','utf8');",
        "const fs = require('fs'); let yield = fs.readFileSync('a','utf8');",
        "const fs = require('fs'); let implements = fs.readFileSync('a','utf8');",
    ] {
        let description = describe(source)?;
        assert_eq!(
            description.disposition(),
            SourceDescriptionDisposition::Fallback(SourceDescriptionFallback::UnsupportedSource)
        );
        assert!(description.references().is_empty());
        assert!(description.sources().is_empty());
    }
    Ok(())
}
