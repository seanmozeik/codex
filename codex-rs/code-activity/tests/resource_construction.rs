//! Direct expressions and call arguments cannot bypass abstract-value budgets.

use codex_code_activity::Analyzer;
use codex_code_activity::Language;
use codex_code_activity::Request;
use codex_code_activity::policy::MockExecutor;
use codex_code_activity::policy::MockPermission;
use codex_code_activity::policy::PolicyDisposition;
use codex_code_activity::policy::review_script;
use pretty_assertions::assert_eq;

#[test]
fn aggregate_alias_expansion_abstains_before_a_binding_is_required() -> eyre::Result<()> {
    let payload = "a".repeat(8_000);
    for (language, source) in [
        (Language::Python, format!("p='{payload}'\n[p,p,p]")),
        (Language::Python, format!("p='{payload}'\nprint(p,p,p)")),
        (
            Language::Python,
            format!("p='{payload}'\nprint(a=p,b=p,c=p)"),
        ),
        (
            Language::Python,
            format!("p='{payload}'\n{{'a':p,'b':p,'c':p}}"),
        ),
        (
            Language::TypeScript,
            format!("const p='{payload}';[p,p,p];"),
        ),
        (
            Language::TypeScript,
            format!("const p='{payload}';console.log(p,p,p);"),
        ),
        (
            Language::TypeScript,
            format!("const p='{payload}';({{a:p,b:p,c:p}});"),
        ),
        (
            Language::TypeScript,
            format!("const p='{payload}';const q=p;const r=p;({{p,q,r}});"),
        ),
    ] {
        let mut analyzer = Analyzer::new()?;
        let reviewed = review_script(
            &mut analyzer,
            Request::new("aggregate-budget".into(), language, source, None),
        );
        assert!(
            reviewed
                .report()
                .unresolved
                .iter()
                .any(|gap| gap.reason.contains("construction budget")),
            "{:?}",
            reviewed.report(),
        );
        assert_eq!(reviewed.decision().disposition, PolicyDisposition::Escalate);
        let mut executor = MockExecutor::default();
        executor.dispatch(&reviewed, MockPermission::Granted);
        assert_eq!(executor.calls(), 0);
    }
    Ok(())
}

#[test]
fn oversized_direct_values_abstain_without_a_binding() -> eyre::Result<()> {
    let payload = "a".repeat(16_384);
    for (language, source) in [
        (Language::Python, format!("'{payload}'")),
        (Language::TypeScript, format!("'{payload}';")),
        (Language::TypeScript, format!("({{'{payload}':1}});")),
    ] {
        let report =
            Analyzer::new()?.analyze(Request::new("direct-budget".into(), language, source, None));
        assert!(
            report
                .unresolved
                .iter()
                .any(|gap| gap.reason.contains("construction budget")),
            "{report:?}"
        );
    }
    Ok(())
}
