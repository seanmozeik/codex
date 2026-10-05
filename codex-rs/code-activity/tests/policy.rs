//! Whole-script mock permission pipeline; adversarial source never executes.

use codex_code_activity::Analyzer;
use codex_code_activity::Language;
use codex_code_activity::MAX_SOURCE_BYTES;
use codex_code_activity::Request;
use codex_code_activity::policy::MockExecutor;
use codex_code_activity::policy::MockPermission;
use codex_code_activity::policy::PolicyDisposition;
use codex_code_activity::policy::PolicyRule;
use codex_code_activity::policy::review_script;
use pretty_assertions::assert_eq;

#[test]
fn later_dangerous_operation_blocks_earlier_benign_operation_before_dispatch() -> anyhow::Result<()>
{
    let fixtures = [
        (
            Language::Shell,
            "cat notes.md; rm -rf /",
            PolicyRule::RootDeletion,
        ),
        (
            Language::Python,
            "from pathlib import Path\nPath('notes.md').read_text()\np='.env'\nPath(p).write_text('secret')",
            PolicyRule::EnvFile,
        ),
        (
            Language::TypeScript,
            "import fs from 'node:fs'; fs.readFileSync('notes.md'); const save=(p: string)=>fs.writeFileSync(p,'secret'); save('.env');",
            PolicyRule::EnvFile,
        ),
        (
            Language::Shell,
            "node -e 'const fs=require(\"fs\"); fs.readFileSync(\"notes.md\"); fs.readFileSync(\".ssh/id_rsa\");'",
            PolicyRule::ReadSsh,
        ),
        (
            Language::TypeScript,
            "await tools.exec_command({cmd:\"cat notes.md; git push --force origin topic\"});",
            PolicyRule::GitForcePush,
        ),
    ];
    let mut analyzer = Analyzer::new()?;
    for (language, source, rule) in fixtures {
        let reviewed = review_script(
            &mut analyzer,
            Request::new(
                "whole-script".into(),
                language,
                source.into(),
                /*cwd*/ None,
            ),
        );
        assert_eq!(reviewed.source(), Some(source));
        assert_eq!(
            reviewed.decision().disposition,
            PolicyDisposition::Block,
            "{source}: {reviewed:?}"
        );
        assert!(
            reviewed.decision().hits.iter().any(|hit| hit.rule == rule),
            "{source}: {reviewed:?}"
        );
        let mut executor = MockExecutor::default();
        executor.dispatch(&reviewed, MockPermission::Granted);
        assert_eq!(executor.calls(), 0, "no earlier portion may dispatch");
    }
    Ok(())
}

#[test]
fn uncertain_opaque_recursive_and_malformed_sources_escalate_without_dispatch() -> anyhow::Result<()>
{
    let fixtures = [
        (
            Language::Python,
            "from pathlib import Path\nPath(dynamic).read_text()",
        ),
        (Language::Python, "def f():\n    return f()\nf()"),
        (
            Language::TypeScript,
            "import fs from 'node:fs'; fs.writeFileSync(dynamic,'x');",
        ),
        (Language::Shell, "python3 -c 'exec(dynamic)'"),
        (Language::Shell, "cat notes.md; echo 'unfinished"),
        (Language::Python, "# no recognized operations"),
    ];
    let mut analyzer = Analyzer::new()?;
    for (language, source) in fixtures {
        let reviewed = review_script(
            &mut analyzer,
            Request::new("unknown".into(), language, source.into(), /*cwd*/ None),
        );
        assert_eq!(
            reviewed.decision().disposition,
            PolicyDisposition::Escalate,
            "{source}: {reviewed:?}"
        );
        let mut executor = MockExecutor::default();
        executor.dispatch(&reviewed, MockPermission::Granted);
        assert_eq!(executor.calls(), 0);
    }
    Ok(())
}

#[test]
fn absence_of_selected_rules_never_supplies_permission() -> anyhow::Result<()> {
    let reviewed = review_script(
        &mut Analyzer::new()?,
        Request::new(
            "ordinary".into(),
            Language::Python,
            "open('notes.md').read()".into(),
            /*cwd*/ None,
        ),
    );
    assert_eq!(
        reviewed.decision().disposition,
        PolicyDisposition::RequireAuthoritativePermission
    );
    assert!(reviewed.decision().hits.is_empty());
    let mut executor = MockExecutor::default();
    executor.dispatch(&reviewed, MockPermission::Absent);
    assert_eq!(executor.calls(), 0);
    executor.dispatch(&reviewed, MockPermission::Granted);
    assert_eq!(
        executor.calls(),
        1,
        "only an independently simulated approval permits a mock call"
    );
    Ok(())
}

#[test]
fn inert_rule_mentions_do_not_create_policy_hits() -> anyhow::Result<()> {
    let source = "from pathlib import Path\n# Path('.env').read_text()\nmessage=\"git push --force; rm -rf /\"\nPath('notes.md').read_text()";
    let reviewed = review_script(
        &mut Analyzer::new()?,
        Request::new(
            "inert".into(),
            Language::Python,
            source.into(),
            /*cwd*/ None,
        ),
    );
    assert!(reviewed.decision().hits.is_empty());
    Ok(())
}

#[test]
fn oversized_raw_source_is_not_copied_or_mock_dispatched() -> anyhow::Result<()> {
    let reviewed = review_script(
        &mut Analyzer::new()?,
        Request::new(
            "large".into(),
            Language::Python,
            "x".repeat(MAX_SOURCE_BYTES + 1),
            /*cwd*/ None,
        ),
    );
    assert_eq!(reviewed.source(), None);
    assert_eq!(reviewed.decision().disposition, PolicyDisposition::Escalate);
    let mut executor = MockExecutor::default();
    executor.dispatch(&reviewed, MockPermission::Granted);
    assert_eq!(executor.calls(), 0);
    Ok(())
}

#[test]
fn force_push_rule_respects_end_of_flags_and_exact_option_names() -> anyhow::Result<()> {
    let mut analyzer = Analyzer::new()?;
    for source in ["git push -- --force", "git push --force-with-leaseBAD"] {
        let reviewed = review_script(
            &mut analyzer,
            Request::new(
                "flags".into(),
                Language::Shell,
                source.into(),
                /*cwd*/ None,
            ),
        );
        assert!(
            reviewed
                .decision()
                .hits
                .iter()
                .all(|hit| hit.rule != PolicyRule::GitForcePush),
            "{source}"
        );
    }
    for source in [
        "git push --force",
        "git push -f",
        "git push --force-with-lease",
        "git push --force-with-lease=topic:abc",
    ] {
        let reviewed = review_script(
            &mut analyzer,
            Request::new(
                "flags".into(),
                Language::Shell,
                source.into(),
                /*cwd*/ None,
            ),
        );
        assert!(
            reviewed
                .decision()
                .hits
                .iter()
                .any(|hit| hit.rule == PolicyRule::GitForcePush),
            "{source}: {reviewed:?}"
        );
    }
    Ok(())
}

#[test]
fn root_deletion_rule_uses_actual_rm_flags_and_direct_language_effects() -> anyhow::Result<()> {
    let mut analyzer = Analyzer::new()?;
    for source in ["rm -- -rf /", "rm -rf ./fixture", "echo 'rm -rf /'"] {
        let reviewed = review_script(
            &mut analyzer,
            Request::new(
                "inert-rm".into(),
                Language::Shell,
                source.into(),
                /*cwd*/ None,
            ),
        );
        assert!(
            reviewed
                .decision()
                .hits
                .iter()
                .all(|hit| hit.rule != PolicyRule::RootDeletion),
            "{source}"
        );
    }
    for (language, source) in [
        (Language::Shell, "rm -rf -- /"),
        (Language::Shell, "rm -fr /"),
        (Language::Python, "import os\nos.remove('/')"),
        (Language::TypeScript, "require('fs').unlinkSync('/');"),
    ] {
        let reviewed = review_script(
            &mut analyzer,
            Request::new("root".into(), language, source.into(), /*cwd*/ None),
        );
        assert!(
            reviewed
                .decision()
                .hits
                .iter()
                .any(|hit| hit.rule == PolicyRule::RootDeletion),
            "{source}: {reviewed:?}"
        );
    }
    Ok(())
}
