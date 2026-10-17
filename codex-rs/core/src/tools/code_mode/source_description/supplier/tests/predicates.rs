//! Conditional supply controls. Source snippets are parsed, never executed.

use super::Fixture;
use super::python;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn owned_read_transform_close_predicates_replay_the_same_target_only() {
    for (expression, before, expected) in [
        (
            "s.replace('red','青',2) if 'red' in s else s",
            "red\r\nred\rred",
            "青\n青\nred",
        ),
        (
            "s.replace('draft','ready') if s == 'draft\\n' else s",
            "draft\n",
            "ready\n",
        ),
        (
            "s[1:-1] if s.startswith('«') and s.endswith('»') else s",
            "«é🧭»",
            "é🧭",
        ),
    ] {
        let fixture = Fixture::new().await;
        fixture.write("file", before.as_bytes()).await;
        let source = format!(
            "with open('file',encoding='utf-8') as h:\n    s=h.read()\nout={expression}\nwith open('file','w',encoding='utf-8') as h:\n    h.write(out)"
        );
        let previews = fixture.supply(&python(&source)).await;
        assert_eq!(previews.len(), 2, "{expression}");
        assert_eq!(previews[0].preview.after_bytes, b"");
        assert_eq!(
            previews[1].preview.before_bytes.as_deref(),
            Some(b"".as_slice())
        );
        assert_eq!(previews[1].preview.after_bytes, expected.as_bytes());
        assert_eq!(previews[1].replay_ordinals, [0]);
        assert_eq!(previews[1].preview.read_bindings.len(), 1);
        assert_eq!(
            previews[1].preview.read_bindings[0].filesystem_epoch.get(),
            0
        );
        assert_eq!(fixture.bytes("file").await, before.as_bytes());
    }
}

#[tokio::test]
async fn conjunction_supplies_only_selected_reads_without_waiving_source_completion() {
    let fixture = Fixture::new().await;
    fixture.write("file", b"plain").await;
    for (left, selected) in [("s == 'other'", false), ("s == 'plain'", true)] {
        let source = format!(
            "from pathlib import Path\ns=Path('file').read_text(encoding='utf-8')\nother=Path('missing').read_text(encoding='utf-8')\nPath('file').write_text('yes' if {left} and other == 'needed' else 'no',encoding='utf-8')"
        );
        let previews = fixture.supply(&python(&source)).await;
        if selected {
            assert!(previews.is_empty());
        } else {
            assert_eq!(previews.len(), 1);
            assert_eq!(previews[0].preview.after_bytes, b"no");
            assert_eq!(previews[0].preview.read_bindings.len(), 1);
            assert_eq!(previews[0].preview.read_bindings[0].read.read_id, 0);
            assert!(!previews[0].preview.predecessor_conditions.is_empty());
        }
    }
    assert_eq!(fixture.bytes("file").await, b"plain");
}
