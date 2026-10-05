//! Inspectable mock policy consumer. This never authorizes real execution.
//!
//! Selected lexical rules are adapted from MIT-licensed Tripwire 0.11.1,
//! commit 56f18dad3ac729db75e6d2437fa74b1b9a3c900f. See NOTICE and
//! `docs/output-contract.md` for exact provenance and deliberately reduced scope.

use crate::ActivityContract;
use crate::Analyzer;
use crate::ArgumentKnowledge;
use crate::ContractAnalysis;
use crate::Coverage;
use crate::Effect;
use crate::MAX_SOURCE_BYTES;
use crate::OperationId;
use crate::Report;
use crate::Request;
use crate::Target;
use crate::WriteMode;
use crate::contract::argument_knowledge;
use serde::Serialize;

/// Small demonstrative subset of actual Tripwire rule identities.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum PolicyRule {
    /// Read `.env` or a suffixed variant.
    ReadEnv,
    /// Mutate `.env` or a suffixed variant.
    EnvFile,
    /// Read a path within `.ssh`.
    ReadSsh,
    /// Mutate a path within `.ssh`.
    SshDir,
    /// Delete the literal filesystem root; broader than Tripwire's rm flags.
    RootDeletion,
    /// Exact literal `git push` with a force option.
    GitForcePush,
}

/// A rule hit tied to the original report-local operation and evidence.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct PolicyHit {
    /// Selected Tripwire-derived rule.
    pub rule: PolicyRule,
    /// Operation that produced the hit; its source span stays in the contract.
    pub operation_id: OperationId,
}

/// No variant means static analysis established safety.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum PolicyDisposition {
    /// A selected rule matched; do not dispatch any part of the script.
    Block,
    /// Carrier, target, source or semantic uncertainty requires escalation.
    Escalate,
    /// No selected rule matched; authoritative permission is still mandatory.
    RequireAuthoritativePermission,
}

/// Deterministic mock classification, without inference or runtime authority.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct PolicyDecision {
    /// Whole-script disposition; never an allow verdict.
    pub disposition: PolicyDisposition,
    /// All selected rule hits, in report order.
    pub hits: Vec<PolicyHit>,
}

/// A source-bound, whole-script review produced only by [`review_script`].
#[derive(Debug)]
pub struct ReviewedScript {
    source: Option<String>,
    report: Report,
    decision: PolicyDecision,
}

impl ReviewedScript {
    /// Original decoded source; oversized inputs are not copied or retained.
    #[must_use]
    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }

    /// Source analysis tied to this exact submitted script.
    #[must_use]
    pub const fn report(&self) -> &Report {
        &self.report
    }

    /// Whole-script mock policy decision, produced before any mock dispatch.
    #[must_use]
    pub const fn decision(&self) -> &PolicyDecision {
        &self.decision
    }
}

/// Parses the entire script, then classifies its source and structured evidence.
///
/// Source/report pairing is private so a caller cannot substitute a different
/// script after review. One bounded source copy retains classifier context;
/// oversized source is consumed by the parser's abstention path and escalated.
#[must_use]
pub fn review_script(analyzer: &mut Analyzer, request: Request) -> ReviewedScript {
    let source = (request.source.len() <= MAX_SOURCE_BYTES).then(|| request.source.clone());
    let report = analyzer.analyze(request);
    let decision = source.as_ref().map_or_else(
        || PolicyDecision {
            disposition: PolicyDisposition::Escalate,
            hits: Vec::new(),
        },
        |script| classify_script(script, &ActivityContract::from_report(&report)),
    );
    ReviewedScript {
        source,
        report,
        decision,
    }
}

/// Inspects the complete raw source and all reported operations before dispatch.
///
/// This mock uses literal structured targets/argv, not raw substring detection.
/// Raw source is retained as classifier context and checked against outer spans.
/// It does not resolve aliases, symlinks, remote paths or nested shell policies.
/// Unknown targets, unsupported carriers, opaque reports and explicit gaps
/// escalate. Even a gap-free partial report requires authoritative permission.
fn classify_script(script: &str, activity: &ActivityContract<'_>) -> PolicyDecision {
    let ContractAnalysis::Supported {
        coverage,
        records,
        unresolved,
        ..
    } = &activity.analysis
    else {
        return PolicyDecision {
            disposition: PolicyDisposition::Escalate,
            hits: Vec::new(),
        };
    };
    let mut uncertain = *coverage == Coverage::Opaque || !unresolved.is_empty();
    let mut hits = Vec::new();
    for operation in *records {
        uncertain |= argument_knowledge(&operation.effect) == ArgumentKnowledge::Unknown;
        uncertain |= matches!(
            operation.effect,
            Effect::FileOpen {
                mode: WriteMode::Unknown,
                ..
            } | Effect::FileWrite {
                mode: WriteMode::Unknown,
                ..
            }
        );
        if operation.evidence.source_id.index() == 0 {
            uncertain |= script
                .get(operation.evidence.start_byte..operation.evidence.end_byte)
                .is_none();
        }
        if let Some(rule) = effect_rule(&operation.effect) {
            hits.push(PolicyHit {
                rule,
                operation_id: operation.id,
            });
        }
    }
    PolicyDecision {
        disposition: if !hits.is_empty() {
            PolicyDisposition::Block
        } else if uncertain {
            PolicyDisposition::Escalate
        } else {
            PolicyDisposition::RequireAuthoritativePermission
        },
        hits,
    }
}

fn effect_rule(effect: &Effect) -> Option<PolicyRule> {
    let (target, read) = match effect {
        Effect::FileDelete {
            target: Target::Literal { path, .. },
        } if path == "/" => {
            return Some(PolicyRule::RootDeletion);
        }
        Effect::FileRead { target } => (target, true),
        Effect::FileOpen { target, .. }
        | Effect::FileWrite { target, .. }
        | Effect::FileEdit { target, .. }
        | Effect::FileTruncate { target }
        | Effect::FileDelete { target } => (target, false),
        Effect::FileList { .. } => return None,
        Effect::ProcessRun { argv, .. } => return process_rule(argv),
    };
    let Target::Literal { path, .. } = target else {
        return None;
    };
    let filename = path.rsplit('/').next().unwrap_or_default();
    if filename == ".env" || filename.starts_with(".env.") {
        Some(if read {
            PolicyRule::ReadEnv
        } else {
            PolicyRule::EnvFile
        })
    } else if path.split('/').any(|part| part == ".ssh") {
        Some(if read {
            PolicyRule::ReadSsh
        } else {
            PolicyRule::SshDir
        })
    } else {
        None
    }
}

fn process_rule(argv: &[Option<String>]) -> Option<PolicyRule> {
    let [Some(command), Some(subcommand), flags @ ..] = argv else {
        return None;
    };
    let executable = command.rsplit('/').next();
    if executable == Some("rm") {
        let operands = &argv[1..];
        let recursive_force = operands
            .iter()
            .flatten()
            .take_while(|flag| flag.as_str() != "--")
            .any(|flag| matches!(flag.as_str(), "-rf" | "-fr" | "-Rf" | "-fR"));
        return (recursive_force && operands.iter().flatten().any(|arg| arg == "/"))
            .then_some(PolicyRule::RootDeletion);
    }
    if executable != Some("git") || subcommand != "push" {
        return None;
    }
    flags
        .iter()
        .flatten()
        .take_while(|flag| flag.as_str() != "--")
        .any(|flag| {
            flag == "-f"
                || flag == "--force"
                || flag == "--force-with-lease"
                || flag.starts_with("--force-with-lease=")
        })
        .then_some(PolicyRule::GitForcePush)
}

/// Explicit test-only stand-in for an independent host permission result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MockPermission {
    /// A fixture simulates authoritative approval, never real host permission.
    Granted,
    /// No independent approval was supplied.
    Absent,
}

/// Counting executor double; never launches a process or accesses files.
#[derive(Debug, Default)]
pub struct MockExecutor {
    calls: usize,
}

impl MockExecutor {
    /// Records one whole-script mock dispatch only after independent approval.
    /// Blocked or escalated scripts remain undispatched even with approval.
    pub fn dispatch(&mut self, script: &ReviewedScript, permission: MockPermission) {
        if script.decision.disposition == PolicyDisposition::RequireAuthoritativePermission
            && permission == MockPermission::Granted
        {
            self.calls += 1;
        }
    }

    /// Number of mock whole-script dispatches; no source has executed.
    #[must_use]
    pub const fn calls(&self) -> usize {
        self.calls
    }
}
