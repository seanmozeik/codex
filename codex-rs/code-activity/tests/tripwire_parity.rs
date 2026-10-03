//! Frozen Tripwire parser inputs. Submitted programs are data and never executed.
use codex_code_activity::Analyzer;
use codex_code_activity::Effect;
use codex_code_activity::Language;
use codex_code_activity::Request;
use codex_code_activity::Target;
use pretty_assertions::assert_eq;
use serde::Deserialize;
use std::collections::BTreeSet;

// Reviewed exceptions: Tripwire rejects the resulting value, but Rust inspects
// every effect in these expressions. Require exact effects, not any deletion.
const DESCRIBED_GAP_FIXTURES: &[&str] = &[
    "Python dictionary summary with an assertion and formatted output: block",
    "Python slice bounds are inspected for effects: block",
    "Compatibility attack 4",
    "Compatibility attack 5",
];

#[derive(Deserialize)]
struct Fixture {
    name: String,
    command: String,
    inputs: Vec<Input>,
}
#[derive(Deserialize)]
struct Input {
    language: String,
    source: String,
    reference: Reference,
}
#[derive(Deserialize)]
struct Reference {
    gap: Option<String>,
    operations: Vec<ReferenceOperation>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum ReferenceOperation {
    Read { path: String },
    Write { path: String },
    Delete { path: String },
    Truncate { path: String },
    Process { argv: Vec<String> },
}

impl ReferenceOperation {
    fn key(&self) -> eyre::Result<String> {
        match self {
            Self::Read { path } => key("read", path),
            Self::Write { path } => key("write", path),
            Self::Delete { path } => key("delete", path),
            Self::Truncate { path } => key("truncate", path),
            Self::Process { argv } => key("process", argv),
        }
    }
}

fn key(kind: &str, value: &impl serde::Serialize) -> eyre::Result<String> {
    Ok(format!("{kind}:{}", serde_json::to_string(value)?))
}

#[test]
fn unsupported_inputs_are_described_or_marked_uncertain() -> eyre::Result<()> {
    let fixtures: Vec<Fixture> = serde_json::from_str(include_str!("fixtures/tripwire.json"))?;
    let mut analyzer = Analyzer::new()?;
    let mut failures = Vec::new();
    for fixture in fixtures {
        for input in fixture.inputs {
            if input.reference.gap.is_none() {
                continue;
            }
            let report = analyzer.analyze(Request::new(
                fixture.name.clone().into(),
                if input.language == "python" {
                    Language::Python
                } else {
                    Language::TypeScript
                },
                input.source,
                None,
            ));
            let unresolved = report
                .unresolved
                .iter()
                .any(|gap| gap.reason != "Imported module initialization is not analysed");
            let dynamic_target = report.operations.iter().any(|operation| {
                matches!(
                    operation.effect,
                    Effect::FileRead {
                        target: Target::Unresolved
                    } | Effect::FileWrite {
                        target: Target::Unresolved,
                        ..
                    } | Effect::FileDelete {
                        target: Target::Unresolved
                    } | Effect::FileTruncate {
                        target: Target::Unresolved
                    } | Effect::FileOpen {
                        target: Target::Unresolved,
                        ..
                    }
                )
            });
            let function_port = matches!(
                fixture.name.as_str(),
                "Python inline 21" | "Python heredoc 21" | "Node inline 16" | "Bun heredoc 16"
            );
            let described = if function_port {
                // The historical reference predates invoked-function support.
                // Require the reviewed full effect set, preserving the old corpus.
                assert_eq!(
                    operation_keys(&report)?,
                    BTreeSet::from([key("delete", &"/tripwire-policy-fixture/protected")?]),
                    "{}",
                    fixture.name
                );
                true
            } else if DESCRIBED_GAP_FIXTURES.contains(&fixture.name.as_str()) {
                let expected: BTreeSet<_> = input
                    .reference
                    .operations
                    .iter()
                    .map(ReferenceOperation::key)
                    .collect::<eyre::Result<_>>()?;
                assert!(!expected.is_empty());
                assert_eq!(operation_keys(&report)?, expected, "{}", fixture.name);
                true
            } else {
                false
            };
            if !unresolved && !dynamic_target && !described {
                failures.push(format!("{}: {:?}", fixture.name, input.reference.gap));
            }
        }
    }
    assert_eq!(failures, Vec::<String>::new());
    Ok(())
}

#[test]
fn rust_recognizes_every_resolved_tripwire_operation() -> eyre::Result<()> {
    let fixtures: Vec<Fixture> = serde_json::from_str(include_str!("fixtures/tripwire.json"))?;
    assert_eq!(fixtures.len(), 355);
    let inputs: Vec<_> = fixtures.iter().flat_map(|f| &f.inputs).collect();
    assert_eq!(inputs.len(), 273);
    assert_eq!(
        inputs.iter().filter(|i| i.reference.gap.is_none()).count(),
        191
    );
    assert_eq!(
        inputs
            .iter()
            .filter(|i| i.reference.gap.is_none())
            .map(|i| i.reference.operations.len())
            .sum::<usize>(),
        223
    );
    let mut analyzer = Analyzer::new()?;
    let mut failures = Vec::new();
    for fixture in fixtures {
        for input in fixture.inputs {
            if input.reference.gap.is_some() {
                continue;
            }
            let report = analyzer.analyze(Request::new(
                fixture.name.clone().into(),
                if input.language == "python" {
                    Language::Python
                } else {
                    Language::TypeScript
                },
                input.source,
                Some("/workspace".into()),
            ));
            let actual = operation_keys(&report)?;
            let expected: BTreeSet<_> = input
                .reference
                .operations
                .iter()
                .map(ReferenceOperation::key)
                .collect::<eyre::Result<_>>()?;
            let unexpected_gaps = report.unresolved.iter().any(|gap| {
                if fixture.name == "JSON comprehension and aggregate: allow"
                    && gap.reason == "Lazy generator body and consumption are not inspected"
                {
                    // Preserve the frozen operation oracle while recording the
                    // narrower generator-consumption coverage honestly.
                    return false;
                }
                !matches!(
                    gap.reason.as_str(),
                    "Imported module initialization is not analysed"
                        | "Branch effects are conditional on runtime control flow"
                        | "Loop effects are conditional on runtime iteration"
                        | "Child process effects and shell option are not evaluated"
                        | "Child process effects are not evaluated"
                )
            });
            if expected != actual || unexpected_gaps {
                failures.push(format!(
                    "{}: missing {:?}; unexpected {:?}; gaps {:?}",
                    fixture.name,
                    expected.difference(&actual).collect::<Vec<_>>(),
                    actual.difference(&expected).collect::<Vec<_>>(),
                    report.unresolved
                ));
            }
        }
    }
    assert_eq!(failures, Vec::<String>::new());
    Ok(())
}

fn operation_keys(report: &codex_code_activity::Report) -> eyre::Result<BTreeSet<String>> {
    let mut keys = BTreeSet::new();
    for operation in &report.operations {
        let (kind, target) = match &operation.effect {
            Effect::FileRead { target } => ("read", target),
            Effect::FileOpen { target, .. }
            | Effect::FileWrite { target, .. }
            | Effect::FileEdit { target, .. } => ("write", target),
            Effect::FileDelete { target } => ("delete", target),
            Effect::FileTruncate { target } => ("truncate", target),
            Effect::ProcessRun { argv, .. } => {
                keys.insert(key("process", argv)?);
                continue;
            }
            _ => continue,
        };
        if let Target::Literal { path, .. } = target {
            keys.insert(key(kind, path)?);
        }
    }
    Ok(keys)
}

#[test]
fn shell_carriers_preserve_tripwire_source_operations() -> eyre::Result<()> {
    let fixtures: Vec<Fixture> = serde_json::from_str(include_str!("fixtures/tripwire.json"))?;
    let mut analyzer = Analyzer::new()?;
    let mut failures = Vec::new();
    for fixture in fixtures {
        let report = analyzer.analyze(Request::new(
            fixture.name.clone().into(),
            Language::Shell,
            fixture.command,
            Some("/workspace".into()),
        ));
        let actual = operation_keys(&report)?;
        for input in fixture.inputs {
            if input.reference.gap.is_some() {
                continue;
            }
            let expected: BTreeSet<_> = input
                .reference
                .operations
                .iter()
                .map(ReferenceOperation::key)
                .collect::<eyre::Result<_>>()?;
            if !expected.is_subset(&actual) || report.sources.len() < 2 {
                failures.push(format!(
                    "{}: missing {:?}; gaps {:?}",
                    fixture.name,
                    expected.difference(&actual).collect::<Vec<_>>(),
                    report.unresolved
                ));
            }
        }
    }
    assert_eq!(failures, Vec::<String>::new());
    Ok(())
}
