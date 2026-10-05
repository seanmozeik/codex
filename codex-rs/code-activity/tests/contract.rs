//! Outbound contract and consumer fixtures; submitted programs remain inert.

use codex_code_activity::ActivityContract;
use codex_code_activity::Analyzer;
use codex_code_activity::ArgumentKnowledge;
use codex_code_activity::Basis;
use codex_code_activity::ContractAnalysis;
use codex_code_activity::ContractSchema;
use codex_code_activity::Effect;
use codex_code_activity::Language;
use codex_code_activity::Request;
use codex_code_activity::Target;
use codex_code_activity::UiAction;
use codex_code_activity::WriteMode;
use codex_code_activity::analyze_codex_command;
use codex_protocol::items::CommandExecutionItem;
use codex_protocol::items::CommandExecutionStatus;
use codex_protocol::protocol::ExecCommandSource;
use codex_shell_command::parse_command::parse_command;
use codex_utils_path_uri::PathUri;
use pretty_assertions::assert_eq;

#[test]
fn envelope_preserves_legacy_records_and_exposes_versioned_shape() -> anyhow::Result<()> {
    let mut analyzer = Analyzer::new()?;
    let report = analyzer.analyze(Request::new(
        "contract-call".into(),
        Language::Python,
        "from pathlib import Path\nPath('a').read_text()\nPath('a').read_text()".into(),
        /*cwd*/ None,
    ));
    let legacy = serde_json::to_value(&report)?;
    let contract = ActivityContract::from_report(&report);
    let value = serde_json::to_value(&contract)?;
    assert_eq!(value["schema"], "codex.codeActivity.v1");
    assert_eq!(value["callId"], "contract-call");
    assert_eq!(value["analysis"]["type"], "supported");
    assert_eq!(value["analysis"]["coverage"], "partial");
    assert_eq!(value["analysis"]["records"], legacy["operations"]);
    assert_eq!(value["analysis"]["sources"], legacy["sources"]);
    assert_eq!(value["analysis"]["unresolved"], legacy["unresolved"]);
    let rows = contract.ui_rows();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].operation_id.index(), 0);
    assert_eq!(rows[1].operation_id.index(), 1);
    assert!(
        rows.iter()
            .all(|row| row.action == UiAction::Read && row.basis == Basis::StaticIntent)
    );
    assert_eq!(serde_json::to_value(&report)?, legacy);
    Ok(())
}

#[test]
fn unknown_contract_version_is_rejected_instead_of_assumed_compatible() {
    assert!(serde_json::from_str::<ContractSchema>("\"codex.codeActivity.v2\"").is_err());
}

#[test]
fn schema_artifact_matches_emitted_variants_and_resource_caps() -> anyhow::Result<()> {
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/output-contract.schema.json"))?;
    assert_eq!(
        schema["properties"]["schema"]["const"],
        "codex.codeActivity.v1"
    );
    let supported = &schema["properties"]["analysis"]["oneOf"][0]["properties"];
    assert_eq!(supported["sources"]["maxItems"], 64);
    assert_eq!(supported["records"]["maxItems"], 256);
    assert_eq!(supported["unresolved"]["maxItems"], 256);
    let target = Target::Literal {
        path: "fixture".into(),
        cwd: None,
    };
    let effects = [
        Effect::FileRead {
            target: target.clone(),
        },
        Effect::FileList {
            target: target.clone(),
        },
        Effect::FileOpen {
            target: target.clone(),
            mode: WriteMode::Replace,
        },
        Effect::FileWrite {
            target: target.clone(),
            mode: WriteMode::Append,
        },
        Effect::FileEdit {
            target: target.clone(),
            transforms: Vec::new(),
        },
        Effect::FileTruncate {
            target: target.clone(),
        },
        Effect::FileDelete { target },
        Effect::ProcessRun {
            argv: vec![Some("fixture".into()), None],
            shell: None,
        },
    ];
    let branches = schema["$defs"]["effect"]["oneOf"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("missing effect union"))?;
    assert_eq!(branches.len(), effects.len());
    for effect in effects {
        let value = serde_json::to_value(effect)?;
        let branch = branches
            .iter()
            .find(|branch| branch["properties"]["type"]["const"] == value["type"])
            .ok_or_else(|| anyhow::anyhow!("schema lacks emitted effect {value}"))?;
        let emitted = value
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("effect is not an object"))?;
        let properties = branch["properties"]
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("schema lacks properties"))?;
        assert_eq!(
            emitted.keys().collect::<Vec<_>>(),
            properties.keys().collect::<Vec<_>>()
        );
    }
    Ok(())
}

#[test]
fn representative_languages_flow_through_records_to_typed_ui_rows() -> anyhow::Result<()> {
    let programs = [
        (
            Language::Python,
            "from pathlib import Path\ndef make(p):\n    return lambda: Path(p).read_text()\nf=make('notes.md')\nf()",
            UiAction::Read,
        ),
        (
            Language::TypeScript,
            "import fs from 'node:fs'; ['a','b'].map((p: string)=>fs.readFileSync(p));",
            UiAction::Read,
        ),
        (
            Language::TypeScript,
            "const fs=require('node:fs'); function save(p) { return ()=>fs.writeFileSync(p,'x'); } save('a')();",
            UiAction::Write,
        ),
        (
            Language::Shell,
            "node -e 'const fs=require(\"node:fs\"); fs.appendFileSync(\"a\",\"x\");'",
            UiAction::Append,
        ),
        (
            Language::Shell,
            "python3 -c 'from pathlib import Path; Path(\"a\").unlink()'",
            UiAction::Delete,
        ),
        (
            Language::TypeScript,
            "await tools.exec_command({cmd:\"python3 -c \\\"open('a').read()\\\"\"});",
            UiAction::Read,
        ),
    ];
    let mut analyzer = Analyzer::new()?;
    for (language, source, expected) in programs {
        let report = analyzer.analyze(Request::new(
            "fixture".into(),
            language,
            source.into(),
            /*cwd*/ None,
        ));
        let contract = ActivityContract::from_report(&report);
        let emitted = serde_json::to_vec(&contract)?;
        let decoded: serde_json::Value = serde_json::from_slice(&emitted)?;
        assert_eq!(decoded["schema"], "codex.codeActivity.v1");
        let rows = contract.ui_rows();
        assert!(
            rows.iter().any(|row| row.action == expected),
            "missing {expected:?}: {source}: {report:?}"
        );
        assert!(rows.iter().all(|row| row.basis == Basis::StaticIntent));
        for row in rows {
            assert!(row.evidence.source_id.index() < report.sources.len());
            assert!(row.evidence.start_byte <= row.evidence.end_byte);
        }
    }
    Ok(())
}

#[test]
fn unknown_targets_and_gaps_remain_visible_even_with_known_rows() -> anyhow::Result<()> {
    let report = Analyzer::new()?.analyze(Request::new(
        "unknown".into(),
        Language::Python,
        "open('known').read()\nopen(dynamic, 'w')\nopaque()".into(),
        /*cwd*/ None,
    ));
    let contract = ActivityContract::from_report(&report);
    assert!(
        contract
            .ui_rows()
            .iter()
            .any(|row| row.argument_knowledge == ArgumentKnowledge::Unknown)
    );
    assert!(
        matches!(&contract.analysis, ContractAnalysis::Supported { unresolved, .. } if !unresolved.is_empty())
    );
    Ok(())
}

#[test]
fn repeated_analysis_emits_identical_bytes_and_report_local_ids() -> anyhow::Result<()> {
    let mut analyzer = Analyzer::new()?;
    let source = "node -e 'require(\"fs\").readFileSync(\"α.txt\")'; cat tail.txt";
    let first = analyzer.analyze(Request::new(
        "same".into(),
        Language::Shell,
        source.into(),
        /*cwd*/ None,
    ));
    let second = analyzer.analyze(Request::new(
        "same".into(),
        Language::Shell,
        source.into(),
        /*cwd*/ None,
    ));
    assert_eq!(
        serde_json::to_vec(&ActivityContract::from_report(&first))?,
        serde_json::to_vec(&ActivityContract::from_report(&second))?
    );
    Ok(())
}

fn command_item(command: Vec<String>) -> anyhow::Result<CommandExecutionItem> {
    Ok(CommandExecutionItem {
        sandbox_type: None,
        model_context: None,
        id: "real-core-item".into(),
        plugin_id: None,
        script_path: None,
        process_id: Some("process".into()),
        parsed_cmd: parse_command(&command),
        command,
        cwd: PathUri::parse("file:///workspace")?,
        source: ExecCommandSource::Agent,
        interaction_input: None,
        status: CommandExecutionStatus::Completed,
        aggregated_output: Some("done".into()),
        exit_code: Some(0),
        duration: None,
    })
}

#[test]
fn actual_codex_item_remains_independent_of_consumer_contract() -> anyhow::Result<()> {
    let item = command_item(
        ["bash", "-c", "python3 -c 'open(\"a\").read()'"]
            .map(str::to_owned)
            .to_vec(),
    )?;
    let before = serde_json::to_value(&item)?;
    let activity = analyze_codex_command(&mut Analyzer::new()?, &item);
    let contract = ActivityContract::from_codex(&activity);
    assert!(
        contract
            .ui_rows()
            .iter()
            .any(|row| row.action == UiAction::Read)
    );
    assert!(
        contract
            .ui_rows()
            .iter()
            .all(|row| row.basis == Basis::StaticIntent)
    );
    assert_eq!(serde_json::to_value(activity.item)?, before);
    assert_eq!(activity.item.aggregated_output.as_deref(), Some("done"));
    Ok(())
}

#[test]
fn unsupported_core_carrier_is_explicit_and_never_an_empty_success() -> anyhow::Result<()> {
    let item = command_item(
        ["pwsh", "-Command", "Get-Content a"]
            .map(str::to_owned)
            .to_vec(),
    )?;
    let activity = analyze_codex_command(&mut Analyzer::new()?, &item);
    let contract = ActivityContract::from_codex(&activity);
    assert!(matches!(
        contract.analysis,
        ContractAnalysis::Unsupported { .. }
    ));
    assert_eq!(
        serde_json::to_value(&contract)?["analysis"]["type"],
        "unsupported"
    );
    assert!(contract.ui_rows().is_empty());
    Ok(())
}
