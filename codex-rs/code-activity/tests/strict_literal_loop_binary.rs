//! Authored binary-provenance controls; no Python source is executed here.
use codex_code_activity::ActivityError;
use codex_code_activity::AnalysisLimits;
use codex_code_activity::Analyzer;
use codex_code_activity::CompleteScript;
use codex_code_activity::Effect;
use codex_code_activity::EnvironmentAssumptions;
use codex_code_activity::FileSnapshot;
use codex_code_activity::Language;
use codex_code_activity::PresentationDisposition;
use codex_code_activity::Request;
use pretty_assertions::assert_eq;
use std::time::Duration;

fn request(source: &str) -> Request {
    Request::new("loop-binary".into(), Language::Python, source.into(), None).with_snapshots(vec![
        FileSnapshot::new("a".into(), Some("old".into()), Some("new".into())),
    ])
}

#[test]
fn binary_origins_anywhere_in_a_loop_source_force_whole_source_fallback()
-> Result<(), ActivityError> {
    for source in [
        "for p in ['a']:\n open(p,'wb').write('text')",
        "for p in ['a']:\n s=open(p,'rb').read()\n open(p,'w').write(s.replace('a','b'))",
        "h=open('a','rb')\nfor p in ['a']:\n h.read()",
        "h=open('a','wb')\nsave=h.write\nfor p in ['a']:\n save('text')",
        "content=open('a','rb').read()\nreplace=content.replace\nfor p in ['a']:\n open(p,'w').write(replace('a','b'))",
        "reader=open\nmode='r'+'b'\nh=reader('a',mode)\nfor p in ['a']:\n h.read()",
        "for p in ['a']:\n open(p,'w'+'b').write('text')",
        "for p in ['a']:\n open(p).read()\nopen('b','wb')",
        "open('a','wb',encoding='utf8')\nfor p in ['a']:\n open(p).read()",
        "h=open('a',mode='rb')\nfor p in ['a']:\n h.read()",
    ] {
        let input = CompleteScript::from_request(
            request(source),
            EnvironmentAssumptions::FreshProcess,
            AnalysisLimits::new(32 * 1024, Duration::from_secs(5)),
        )?;
        let activity = Analyzer::new()?.analyze_complete(input);
        assert!(
            matches!(activity.disposition(), PresentationDisposition::Fallback(_)),
            "{source}"
        );
        assert!(activity.report().operations.is_empty(), "{source}");
        assert!(activity.ui_rows().is_empty(), "{source}");
        assert!(activity.diffs().is_empty(), "{source}");
    }
    Ok(())
}

#[test]
fn text_handles_and_content_from_before_the_loop_remain_supported() -> Result<(), ActivityError> {
    for source in [
        "h=open('a','rt')\nread=h.read\nfor p in ['a']:\n read()",
        "s=open('a').read()\nreplace=s.replace\nfor p in ['a']:\n open(p,'wt').write(replace('old','new'))",
    ] {
        let input = CompleteScript::from_request(
            request(source),
            EnvironmentAssumptions::FreshProcess,
            AnalysisLimits::new(32 * 1024, Duration::from_secs(5)),
        )?;
        assert_eq!(
            Analyzer::new()?.analyze_complete(input).disposition(),
            PresentationDisposition::Eligible
        );
    }
    Ok(())
}

#[test]
fn binary_policy_does_not_change_diagnostic_or_loop_free_analysis() -> Result<(), ActivityError> {
    let without_loop = "open('a','wb').write('text')";
    let input = CompleteScript::from_request(
        request(without_loop),
        EnvironmentAssumptions::FreshProcess,
        AnalysisLimits::new(32 * 1024, Duration::from_secs(5)),
    )?;
    assert_eq!(
        Analyzer::new()?.analyze_complete(input).disposition(),
        PresentationDisposition::Eligible
    );
    let diagnostic =
        Analyzer::new()?.analyze(request("for p in ['a']:\n open(p,'wb').write('text')"));
    assert_eq!(diagnostic.operations.len(), 2);
    assert!(
        !diagnostic
            .unresolved
            .iter()
            .any(|gap| gap.reason.contains("Binary open"))
    );
    Ok(())
}

#[test]
fn non_text_handle_origins_and_aliases_are_rejected_anywhere_in_loop_sources()
-> Result<(), ActivityError> {
    for method in ["readlines", "text", "json"] {
        for source in [
            format!(
                "s=open('a').{method}()\nfor p in ['a']:\n open(p,'w').write(s.replace('old','new'))"
            ),
            format!(
                "load=open('a').{method}\ns=load()\nreplace=s.replace\nfor p in ['a']:\n open(p,'w').write(replace('old','new'))"
            ),
            format!("for p in ['a']:\n open(p).{method}()"),
            format!("load=open('a').{method}\nfor p in ['a']:\n open(p).read()\nload()"),
        ] {
            let input = CompleteScript::from_request(
                request(&source),
                EnvironmentAssumptions::FreshProcess,
                AnalysisLimits::new(32 * 1024, Duration::from_secs(5)),
            )?;
            let activity = Analyzer::new()?.analyze_complete(input);
            assert!(
                matches!(activity.disposition(), PresentationDisposition::Fallback(_)),
                "{source}"
            );
            assert!(activity.report().operations.is_empty(), "{source}");
            assert!(activity.ui_rows().is_empty(), "{source}");
            assert!(activity.diffs().is_empty(), "{source}");
        }
    }
    Ok(())
}

#[test]
fn non_text_read_origin_policy_leaves_other_analysis_modes_unchanged() -> Result<(), ActivityError>
{
    let source = "open('a').readlines()";
    let input = CompleteScript::from_request(
        request(source),
        EnvironmentAssumptions::FreshProcess,
        AnalysisLimits::new(32 * 1024, Duration::from_secs(5)),
    )?;
    assert_eq!(
        Analyzer::new()?.analyze_complete(input).disposition(),
        PresentationDisposition::Eligible
    );
    let report = Analyzer::new()?.analyze(request("for p in ['a']:\n open(p).readlines()"));
    assert_eq!(report.operations.len(), 1);
    assert!(
        !report
            .unresolved
            .iter()
            .any(|gap| gap.reason.contains("Non-text handle read"))
    );
    Ok(())
}

#[test]
fn whole_source_grammar_and_value_proofs_reject_erased_provenance() -> Result<(), ActivityError> {
    for source in [
        "h=str(open('a'))\nfor p in ['b']:\n h.read()",
        "s=open('a').read().slice(0)\nfor p in ['a']:\n open(p,'w').write(s)",
        "s=open('a').read().trim()\nfor p in ['a']:\n open(p,'w').write(s)",
        "s=open('a').read().replaceAll('a','b')\nfor p in ['a']:\n open(p,'w').write(s)",
        "s=open('a').read()['bad':]\nfor p in ['a']:\n open(p,'w').write(s)",
        "with open('a') as h:\n h.read()\nfor p in ['b']:\n h.read()",
        "h=open('a')\nh.close()\nfor p in ['b']:\n h.read()",
        "for p in ['a']:\n h=open(p)\n h.close()\n h.read()",
        "for p in ['a']:\n open(p).read()\nopen('a').close()",
        "missing\nfor p in ['a']:\n open(p).read()",
        "x=missing\nfor p in ['a']:\n open(p).read()",
        "'a'-'b'\nfor p in ['a']:\n open(p).read()",
        "unknown+unknown\nfor p in ['a']:\n open(p).read()",
        "def unused():\n return 'a'\nfor p in ['a']:\n open(p).read()",
    ] {
        let input = CompleteScript::from_request(
            request(source),
            EnvironmentAssumptions::FreshProcess,
            AnalysisLimits::new(32 * 1024, Duration::from_secs(5)),
        )?;
        let activity = Analyzer::new()?.analyze_complete(input);
        assert!(
            matches!(activity.disposition(), PresentationDisposition::Fallback(_)),
            "{source}"
        );
        assert!(activity.report().operations.is_empty(), "{source}");
        assert!(activity.ui_rows().is_empty(), "{source}");
        assert!(activity.diffs().is_empty(), "{source}");
    }
    Ok(())
}

#[test]
fn plain_aliases_prefix_reads_and_sequential_loops_remain_supported() -> Result<(), ActivityError> {
    let source = "reader=open\ns=reader('a').read()\nfor p in ['a']:\n reader(p,'w').write(s.replace('old','new'))\nfor p in ['b']:\n reader(p,'w').write('new')";
    let input = CompleteScript::from_request(
        request(source),
        EnvironmentAssumptions::FreshProcess,
        AnalysisLimits::new(32 * 1024, Duration::from_secs(5)),
    )?;
    let activity = Analyzer::new()?.analyze_complete(input);
    assert_eq!(activity.disposition(), PresentationDisposition::Eligible);
    assert_eq!(activity.report().operations.len(), 5);
    Ok(())
}

#[test]
fn content_transform_limits_are_checked_before_silent_saturation() -> Result<(), ActivityError> {
    let items = std::iter::repeat_n("'a'", 16).collect::<Vec<_>>().join(",");
    for expression in ["s.replace('old','new')", "s+'suffix'", "s.strip()"] {
        let boundary = format!(
            "s=open('a').read()\nfor p in [{items}]:\n s={expression}\n s={expression}\nopen('a','w').write(s)"
        );
        let input = CompleteScript::from_request(
            request(&boundary),
            EnvironmentAssumptions::FreshProcess,
            AnalysisLimits::new(32 * 1024, Duration::from_secs(5)),
        )?;
        let activity = Analyzer::new()?.analyze_complete(input);
        assert_eq!(activity.disposition(), PresentationDisposition::Eligible);
        assert!(activity.report().operations.iter().any(|operation|
            matches!(&operation.effect, Effect::FileEdit { transforms, .. } if transforms.len() == 32)));
        let overflow = format!(
            "s=open('a').read()\nfor p in [{items}]:\n s={expression}\n s={expression}\n s={expression}\nopen('a','w').write(s)"
        );
        let input = CompleteScript::from_request(
            request(&overflow),
            EnvironmentAssumptions::FreshProcess,
            AnalysisLimits::new(32 * 1024, Duration::from_secs(5)),
        )?;
        let activity = Analyzer::new()?.analyze_complete(input);
        assert!(matches!(
            activity.disposition(),
            PresentationDisposition::Fallback(_)
        ));
        assert!(activity.report().operations.is_empty());
        assert!(activity.ui_rows().is_empty());
        assert!(activity.diffs().is_empty());
    }
    Ok(())
}
