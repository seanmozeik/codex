//! Authored source is parsed but never executed; only disposable files are read.

mod boundaries;
mod logical;
mod predicates;
mod presentation;

use super::AutomaticSnapshot;
use super::SnapshotSupplier;
use crate::config::PermissionProfileSnapshot;
use crate::session::tests::make_session_and_context;
use crate::session::turn_context::TurnEnvironment;
use crate::tools::code_mode::source_description::convert;
use codex_code_activity as activity;
use codex_file_system::EnvironmentAccess;
use codex_file_system::ReadFileOptions;
use codex_file_system::WriteFileOptions;
use codex_protocol::models::PermissionProfile;
use codex_protocol::source_description::SourceDescription;
use codex_utils_path_uri::PathUri;
use pretty_assertions::assert_eq;
use std::time::Duration;
use std::time::Instant;
use tokio_util::sync::CancellationToken;

const REPLACE: &str = "from pathlib import Path\np = Path('file')\ns = p.read_text(encoding='utf-8')\np.write_text(s.replace('old', 'new'), encoding='utf-8')";

struct Fixture {
    _directory: tempfile::TempDir,
    environment: TurnEnvironment,
}

impl Fixture {
    async fn new() -> Self {
        let (_session, turn) = make_session_and_context().await;
        let mut environment = turn
            .initial_environments
            .primary()
            .cloned()
            .expect("local fixture");
        let directory = tempfile::tempdir().expect("disposable fixture");
        environment.selection.cwd = PathUri::from_host_native_path(directory.path()).expect("cwd");
        environment.config_mut().workspace_roots.clear();
        environment.config_mut().permission_profile =
            PermissionProfileSnapshot::legacy(PermissionProfile::Disabled);
        Self {
            _directory: directory,
            environment,
        }
    }

    async fn write(&self, path: &str, bytes: &[u8]) {
        self.environment
            .fs_accessor(None)
            .write_file(
                &self.environment.cwd().join(path).expect("fixture path"),
                bytes.to_vec(),
                WriteFileOptions::default(),
            )
            .await
            .expect("write authored bytes");
    }

    async fn bytes(&self, path: &str) -> Vec<u8> {
        self.environment
            .fs_accessor(None)
            .read_file(
                &self.environment.cwd().join(path).expect("fixture path"),
                ReadFileOptions::default(),
            )
            .await
            .expect("read fixture bytes")
    }

    async fn supply(&self, analysis: &activity::SelectiveEditAnalysis) -> Vec<AutomaticSnapshot> {
        SnapshotSupplier::new(self.environment.clone())
            .supply(
                analysis,
                &CancellationToken::new(),
                Instant::now() + Duration::from_secs(5),
            )
            .await
    }
}

fn python(source: &str) -> activity::SelectiveEditAnalysis {
    let input = activity::SelectiveEditInput::from_source(
        "supplier-fixture".into(),
        activity::Language::Python,
        source,
        None,
        activity::AnalysisLimits::new(32_768, Duration::from_secs(2)),
    )
    .expect("bounded authored source");
    activity::Analyzer::new()
        .expect("native parser")
        .forecast_edits(input)
}

fn node(source: &str) -> activity::SelectiveEditAnalysis {
    let input = activity::SelectiveEditInput::from_node_commonjs(
        "supplier-fixture".into(),
        source,
        None,
        activity::AnalysisLimits::new(32_768, Duration::from_secs(2)),
    )
    .expect("bounded authored source");
    activity::Analyzer::new()
        .expect("native parser")
        .forecast_edits(input)
}

#[tokio::test]
async fn supplies_real_entry_bytes_without_executing_source() {
    let fixture = Fixture::new().await;
    fixture.write("file", b"old\r\n").await;
    let previews = fixture.supply(&python(REPLACE)).await;
    assert_eq!(previews.len(), 1);
    let value = &previews[0];
    assert_eq!(value.preview.call_id.as_str(), "supplier-fixture");
    assert_eq!(
        value.preview.before_bytes.as_deref(),
        Some(b"old\r\n".as_slice())
    );
    assert_eq!(value.preview.after_bytes, b"new\n");
    assert_eq!(value.preview.read_bindings.len(), 1);
    assert_eq!(value.preview.read_bindings[0].filesystem_epoch.get(), 0);
    assert_eq!(value.resolved_cwd, *fixture.environment.cwd());
    assert!(matches!(
        &value.preview.target,
        activity::Target::Literal { cwd: None, .. }
    ));
    assert_eq!(fixture.bytes("file").await, b"old\r\n");
}

#[tokio::test]
async fn replays_truncate_before_the_following_write() {
    let fixture = Fixture::new().await;
    fixture.write("file", b"original").await;
    let analysis = python("open('file', 'w', encoding='utf-8').write('replacement')");
    let previews = fixture.supply(&analysis).await;
    assert_eq!(previews.len(), 2);
    assert_eq!(previews[0].preview.after_bytes, b"");
    assert_eq!(
        previews[1].preview.before_bytes.as_deref(),
        Some(b"".as_slice())
    );
    assert_eq!(previews[1].preview.after_bytes, b"replacement");
    assert_eq!(previews[1].replay_ordinals, [0]);
    assert_eq!(fixture.bytes("file").await, b"original");
}

#[tokio::test]
async fn preserves_captured_read_across_replayed_mutations() {
    let fixture = Fixture::new().await;
    fixture.write("file", b"entry").await;
    let analysis = python(
        "from pathlib import Path\np = Path('file')\ns = p.read_text(encoding='utf-8')\np.write_text('middle', encoding='utf-8')\np.write_text(s + '!', encoding='utf-8')",
    );
    let previews = fixture.supply(&analysis).await;
    assert_eq!(previews.len(), 2);
    assert_eq!(
        previews[1].preview.before_bytes.as_deref(),
        Some(b"middle".as_slice())
    );
    assert_eq!(previews[1].preview.after_bytes, b"entry!");
    assert_eq!(
        previews[1].preview.read_bindings[0].filesystem_epoch.get(),
        0
    );
}

#[tokio::test]
async fn reads_after_a_modeled_write_use_replayed_bytes() {
    let fixture = Fixture::new().await;
    fixture.write("file", b"entry").await;
    let analysis = python(
        "from pathlib import Path\np = Path('file')\np.write_text('middle', encoding='utf-8')\ns = p.read_text(encoding='utf-8')\np.write_text(s + '!', encoding='utf-8')",
    );
    let previews = fixture.supply(&analysis).await;
    assert_eq!(previews.len(), 2);
    assert_eq!(previews[1].preview.after_bytes, b"middle!");
    assert_eq!(
        previews[1].preview.read_bindings[0].filesystem_epoch.get(),
        1
    );
}

#[tokio::test]
async fn fresh_reads_in_conditional_arms_remain_symbolic() {
    let fixture = Fixture::new().await;
    fixture.write("file", b"old").await;
    let analysis = python(
        "from pathlib import Path\ns = Path('file').read_text(encoding='utf-8')\nPath('file').write_text('selected' if s.startswith('old') else Path('missing').read_text(encoding='utf-8'), encoding='utf-8')",
    );
    assert_eq!(
        analysis.disposition(),
        activity::SelectiveEditDisposition::Analyzed
    );
    assert!(analysis.edits().is_empty());
    assert_eq!(analysis.python_barriers().len(), 1);
    assert_eq!(
        analysis.python_barriers()[0].reason,
        activity::PythonBarrierReason::UnsupportedOperation
    );
    assert!(fixture.supply(&analysis).await.is_empty());
}

#[tokio::test]
async fn captures_only_reads_used_by_the_selected_closed_recipe() {
    let fixture = Fixture::new().await;
    fixture.write("file", b"old").await;
    let source = "from pathlib import Path\ns = Path('file').read_text(encoding='utf-8')\nunused = Path('missing').read_text(encoding='utf-8')\nPath('file').write_text('selected' if s.startswith('old') else unused, encoding='utf-8')";
    let analysis = python(source);
    assert_eq!(
        analysis.disposition(),
        activity::SelectiveEditDisposition::Analyzed
    );
    assert_eq!(analysis.edits().len(), 1);
    assert!(analysis.python_barriers().is_empty());
    let previews = fixture.supply(&analysis).await;
    assert_eq!(previews.len(), 1);
    let preview = &previews[0].preview;
    assert_eq!(preview.after_bytes, b"selected");
    assert_eq!(preview.read_bindings.len(), 1);
    assert_eq!(preview.read_bindings[0].read.read_id, 0);
    // The earlier read must still finish normally to reach this operation.
    // Supplying only selected recipe bytes does not claim that it succeeds.
    assert!(preview.predecessor_conditions.iter().any(|span| {
        source.get(span.start_byte..span.end_byte)
            == Some("Path('missing').read_text(encoding='utf-8')")
    }));
}

#[tokio::test]
async fn node_replacement_preserves_newlines() {
    let fixture = Fixture::new().await;
    fixture.write("file", b"old\r\nold\r\n").await;
    let analysis = node(
        "const fs = require('fs'); const s = fs.readFileSync('file', 'utf8'); fs.writeFileSync('file', s.replace('old', 'new'), 'utf8');",
    );
    let previews = fixture.supply(&analysis).await;
    assert_eq!(previews.len(), 1);
    assert_eq!(previews[0].preview.after_bytes, b"new\r\nold\r\n");
}

#[tokio::test]
async fn node_slice_resolves_its_captured_read_across_replayed_mutation() {
    let fixture = Fixture::new().await;
    fixture.write("file", "🦉tail\n".as_bytes()).await;
    let analysis = node(
        "const fs=require('fs'); const saved=fs.readFileSync('file','utf8'); fs.writeFileSync('file','changed'); fs.writeFileSync('file',saved.slice(0,2)+'!');",
    );
    let previews = fixture.supply(&analysis).await;
    assert_eq!(previews.len(), 2);
    let preview = &previews[1].preview;
    assert_eq!(preview.before_bytes.as_deref(), Some(b"changed".as_slice()));
    assert_eq!(preview.after_bytes, "🦉!".as_bytes());
    assert_eq!(preview.read_bindings.len(), 1);
    assert_eq!(preview.read_bindings[0].filesystem_epoch.get(), 0);
    assert_eq!(fixture.bytes("file").await, "🦉tail\n".as_bytes());
}

#[tokio::test]
async fn node_slice_with_unpaired_surrogate_stays_symbolic_without_a_snapshot() {
    let fixture = Fixture::new().await;
    fixture.write("file", "🦉tail".as_bytes()).await;
    let analysis = node(
        "const fs=require('fs'); const text=fs.readFileSync('file','utf8'); fs.writeFileSync('file',text.slice(0,1));",
    );
    assert_eq!(analysis.edits().len(), 1);
    let previews = fixture.supply(&analysis).await;
    assert!(previews.is_empty());
    let description = convert::forecast(&analysis, &previews);
    let SourceDescription::SelectiveFindings { edits, .. } = description else {
        panic!("symbolic slice remains presentable");
    };
    assert_eq!(edits.len(), 1);
    assert!(edits[0].snapshot.is_none());
    assert_eq!(fixture.bytes("file").await, "🦉tail".as_bytes());
}
