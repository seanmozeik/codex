use super::Fixture;
use super::REPLACE;
use super::node;
use super::python;
use crate::config::PermissionProfileSnapshot;
use crate::tools::code_mode::source_description::supplier::MAX_FILE_BYTES;
use crate::tools::code_mode::source_description::supplier::SnapshotSupplier;
use codex_code_activity as activity;
#[cfg(unix)]
use codex_file_system::CreateDirectoryOptions;
#[cfg(unix)]
use codex_file_system::EnvironmentAccess;
use codex_protocol::models::PermissionProfile;
use codex_protocol::permissions::FileSystemSandboxPolicy;
use codex_protocol::permissions::NetworkSandboxPolicy;
use pretty_assertions::assert_eq;
use std::time::Duration;
use std::time::Instant;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn missing_file_is_absence_but_non_utf8_and_oversize_are_unavailable() {
    let fixture = Fixture::new().await;
    let source = "from pathlib import Path\nPath('file').write_text('new', encoding='utf-8')";
    let analysis = python(source);
    assert_eq!(
        analysis.disposition(),
        activity::SelectiveEditDisposition::Analyzed
    );
    assert_eq!(analysis.edits().len(), 1);
    let previews = fixture.supply(&analysis).await;
    assert_eq!(previews.len(), 1);
    assert_eq!(previews[0].preview.before_bytes, None);
    for bytes in [vec![0xff], vec![b'x'; MAX_FILE_BYTES + 1]] {
        fixture.write("file", &bytes).await;
        assert!(fixture.supply(&analysis).await.is_empty());
    }
}

#[tokio::test]
async fn runtime_default_python_codec_remains_symbolic() {
    let fixture = Fixture::new().await;
    fixture.write("file", b"old").await;
    let source = "from pathlib import Path\np = Path('file')\np.write_text(p.read_text().replace('old', 'new'))";
    assert!(fixture.supply(&python(source)).await.is_empty());
}

#[tokio::test]
async fn foreign_target_after_a_mutation_is_not_stamped_with_fresh_disk_bytes() {
    let fixture = Fixture::new().await;
    fixture.write("file", b"entry").await;
    fixture.write("other", b"other-entry").await;
    let source = "from pathlib import Path\nPath('file').write_text('first', encoding='utf-8')\nPath('other').write_text('second', encoding='utf-8')";
    let previews = fixture.supply(&python(source)).await;
    assert_eq!(previews.len(), 1);
    assert_eq!(previews[0].preview.after_bytes, b"first");
}

#[tokio::test]
async fn different_read_target_after_mutation_is_not_assumed_alias_free() {
    let fixture = Fixture::new().await;
    fixture.write("file", b"entry").await;
    fixture.write("other", b"other-entry").await;
    let source = "from pathlib import Path\np = Path('file')\np.write_text('first', encoding='utf-8')\np.write_text(Path('other').read_text(encoding='utf-8'), encoding='utf-8')";
    let previews = fixture.supply(&python(source)).await;
    assert_eq!(previews.len(), 1);
}

#[tokio::test]
async fn sequential_child_invocation_is_not_an_operation_point_snapshot() {
    let fixture = Fixture::new().await;
    fixture.write("file", b"entry").await;
    let command = serde_json::to_string("python3 - <<'PY'\nfrom pathlib import Path\nPath('file').write_text('new', encoding='utf-8')\nPY\n").expect("authored command");
    let source = format!(
        "await tools.exec_command({{cmd:{command}}}); await tools.exec_command({{cmd:{command}}});"
    );
    let input = activity::SelectiveEditInput::from_source(
        "supplier-fixture".into(),
        activity::Language::TypeScript,
        &source,
        None,
        activity::AnalysisLimits::new(32_768, Duration::from_secs(2)),
    )
    .expect("bounded source");
    let analysis = activity::Analyzer::new()
        .expect("native parser")
        .forecast_edits(input);
    assert_eq!(analysis.edits().len(), 2);
    let previews = fixture.supply(&analysis).await;
    assert_eq!(previews.len(), 1);
}

#[tokio::test]
async fn cancellation_and_elapsed_deadline_do_not_begin_capture() {
    let fixture = Fixture::new().await;
    fixture.write("file", b"old").await;
    let analysis = python(REPLACE);
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let previews = SnapshotSupplier::new(fixture.environment.clone())
        .supply(
            &analysis,
            &cancellation,
            Instant::now() + Duration::from_secs(5),
        )
        .await;
    assert!(previews.is_empty());
    let previews = SnapshotSupplier::new(fixture.environment.clone())
        .supply(
            &analysis,
            &CancellationToken::new(),
            Instant::now() - Duration::from_secs(1),
        )
        .await;
    assert!(previews.is_empty());
    assert_eq!(fixture.bytes("file").await, b"old");
}

#[tokio::test]
async fn sandbox_unavailability_is_not_retried_with_host_filesystem_access() {
    let mut fixture = Fixture::new().await;
    fixture.write("file", b"old").await;
    fixture.environment.config_mut().permission_profile =
        PermissionProfileSnapshot::legacy(PermissionProfile::from_runtime_permissions(
            &FileSystemSandboxPolicy::restricted(Vec::new()),
            NetworkSandboxPolicy::Restricted,
        ));
    assert!(fixture.supply(&python(REPLACE)).await.is_empty());
}

#[tokio::test]
async fn path_normalization_cannot_turn_a_directory_required_operand_into_a_file() {
    let fixture = Fixture::new().await;
    fixture.write("file", b"entry").await;
    for path in ["file/.", "missing/.", "file/", "link/../file"] {
        let source =
            format!("const fs = require('fs'); fs.writeFileSync('{path}', 'new', 'utf8');");
        assert!(fixture.supply(&node(&source)).await.is_empty(), "{path}");
    }
}

#[tokio::test]
async fn descriptor_bytes_follow_replayed_truncation_and_positions() {
    let fixture = Fixture::new().await;
    fixture.write("file", b"original").await;
    let analysis = node(
        "const fs = require('fs'); const fd = fs.openSync('file', 'w'); fs.writeSync(fd, 'abc'); fs.writeSync(fd, 'Z', 1, 'utf8'); fs.closeSync(fd);",
    );
    let previews = fixture.supply(&analysis).await;
    assert_eq!(previews.len(), 3);
    assert_eq!(previews[1].preview.after_bytes, b"abc");
    assert_eq!(previews[2].preview.after_bytes, b"aZc");
    assert_eq!(previews[2].replay_ordinals, [0, 1]);
    assert_eq!(fixture.bytes("file").await, b"original");
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_target_is_captured_but_dangling_symlink_is_not_absence() {
    let fixture = Fixture::new().await;
    fixture.write("actual", b"old").await;
    let directory = fixture._directory.path();
    std::os::unix::fs::symlink("actual", directory.join("file")).expect("authored symlink");
    let previews = fixture.supply(&python(REPLACE)).await;
    assert_eq!(previews.len(), 1);
    assert!(
        previews[0]
            .resolved_target
            .inferred_native_path_string()
            .ends_with("/actual")
    );
    std::os::unix::fs::symlink("absent", directory.join("dangling")).expect("dangling fixture");
    let source = "from pathlib import Path\nPath('dangling').write_text('new', encoding='utf-8')";
    assert!(fixture.supply(&python(source)).await.is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn missing_leaf_below_symlink_parent_is_absence_but_dangling_leaf_is_not() {
    let fixture = Fixture::new().await;
    let actual = fixture
        .environment
        .cwd()
        .join("actual")
        .expect("directory path");
    let access = fixture.environment.fs_accessor(None);
    access
        .create_directory(
            &actual,
            CreateDirectoryOptions {
                recursive: false,
                follow_symlinks: true,
            },
        )
        .await
        .expect("authored directory");
    std::os::unix::fs::symlink("actual", fixture._directory.path().join("link"))
        .expect("authored directory symlink");
    let analysis =
        python("from pathlib import Path\nPath('link/file').write_text('new', encoding='utf-8')");
    assert_eq!(analysis.edits().len(), 1);
    let previews = fixture.supply(&analysis).await;
    assert_eq!(previews.len(), 1);
    assert_eq!(previews[0].preview.before_bytes, None);
    assert_eq!(previews[0].preview.after_bytes, b"new");
    assert_eq!(
        previews[0].resolved_target,
        access
            .canonicalize(&actual)
            .await
            .expect("canonical directory")
            .join("file")
            .expect("missing leaf")
    );
    std::os::unix::fs::symlink("absent", fixture._directory.path().join("actual/dangling"))
        .expect("authored dangling symlink");
    let source =
        "from pathlib import Path\nPath('link/dangling').write_text('new', encoding='utf-8')";
    assert!(fixture.supply(&python(source)).await.is_empty());
}
