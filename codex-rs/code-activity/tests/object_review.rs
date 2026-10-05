//! Skipped object-property evaluation invalidates state its children may mutate.

use codex_code_activity::Analyzer;
use codex_code_activity::Effect;
use codex_code_activity::Language;
use codex_code_activity::Request;
use codex_code_activity::Target;
use codex_code_activity::policy::MockExecutor;
use codex_code_activity::policy::MockPermission;
use codex_code_activity::policy::PolicyDisposition;
use codex_code_activity::policy::review_script;
use pretty_assertions::assert_eq;

#[test]
fn skipped_spread_and_computed_properties_cannot_retain_a_stale_target() -> anyhow::Result<()> {
    for property in ["...change()", "[change()]:1", "[change()](){}"] {
        let source = format!(
            "const fs=require('fs');let p='safe';function change(){{p='.env';return {{}}}} ({{{property}}});fs.writeFileSync(p,'x');"
        );
        let reviewed = review_script(
            &mut Analyzer::new()?,
            Request::new(
                "object-state".into(),
                Language::TypeScript,
                source,
                Some("/original".into()),
            ),
        );
        assert!(
            reviewed
                .report()
                .unresolved
                .iter()
                .any(|gap| gap.reason.contains("object")),
            "{reviewed:?}"
        );
        assert!(
            reviewed.report().operations.is_empty(),
            "a skipped property cannot retain the old fs/p bindings: {reviewed:?}"
        );
        assert_eq!(reviewed.decision().disposition, PolicyDisposition::Escalate);
        let mut executor = MockExecutor::default();
        executor.dispatch(&reviewed, MockPermission::Granted);
        assert_eq!(executor.calls(), 0);
    }
    Ok(())
}

#[test]
fn skipped_object_children_cannot_retain_cwd_after_a_fresh_api_import() -> anyhow::Result<()> {
    for property in ["...change()", "[change()]:1", "[change()](){}"] {
        let source = format!(
            "function change(){{process.chdir('/other');return {{}}}} ({{{property}}});import fs from 'node:fs';fs.writeFileSync('relative','x');"
        );
        let report = Analyzer::new()?.analyze(Request::new(
            "object-cwd".into(),
            Language::TypeScript,
            source,
            Some("/original".into()),
        ));
        assert_eq!(report.operations.len(), 1, "{report:?}");
        assert!(
            matches!(
                &report.operations[0].effect,
                Effect::FileWrite { target: Target::Literal { path, cwd: None }, .. } if path == "relative"
            ),
            "a recovered API cannot recover the stale cwd: {report:?}"
        );
        assert!(!report.unresolved.is_empty());
    }
    Ok(())
}
