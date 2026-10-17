use super::Result;
use super::flow;
use super::input;
use super::input::Completion;
use super::input::Launch;
use codex_protocol::items::CommandExecutionStatus;
use codex_protocol::items::TurnItem;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ExecCommandStatus;
use codex_protocol::protocol::ItemCompletedEvent;
use codex_protocol::protocol::ItemStartedEvent;
use pretty_assertions::assert_eq;
use std::io::Cursor;
use std::io::Write;
use std::time::Duration;

#[test]
fn actual_events_roundtrip_and_stdout_does_not_become_a_file_change() -> Result<()> {
    let root = tempfile::tempdir()?;
    let command = vec!["/bin/bash".into(), "-c".into(), "printf 'edited'".into()];
    let launch = Launch {
        call_id: "real-event-fixture".into(),
        command: command.clone(),
        cwd: root.path().to_string_lossy().into_owned(),
        started_at_ms: 1000,
    };
    let (prepared, ready) = flow::prepare(launch)?;
    let start: ItemStartedEvent = serde_json::from_value(ready["itemStarted"].clone())?;
    let TurnItem::CommandExecution(item) = start.item else {
        panic!("expected command")
    };
    assert_eq!(item.command, command);
    assert_eq!(item.status, CommandExecutionStatus::InProgress);
    assert_eq!(ready["rows"], serde_json::json!([]));
    let begins: Vec<EventMsg> = serde_json::from_value(ready["legacyEvents"].clone())?;
    let [EventMsg::ExecCommandBegin(begin)] = begins.as_slice() else {
        panic!("expected real begin")
    };
    assert_eq!(begin.command, command);
    assert_eq!(begin.started_at_ms, 1000);
    let completed = prepared.complete(Completion {
        exit_code: 0,
        output: "edited".into(),
        duration_micros: 1234,
        completed_at_ms: 1002,
    })?;
    let end: ItemCompletedEvent = serde_json::from_value(completed["itemCompleted"].clone())?;
    let TurnItem::CommandExecution(item) = end.item else {
        panic!("expected command")
    };
    assert_eq!(item.command, command);
    assert_eq!(item.exit_code, Some(0));
    assert_eq!(item.duration, Some(Duration::from_micros(1234)));
    let ends: Vec<EventMsg> = serde_json::from_value(completed["legacyEvents"].clone())?;
    let [EventMsg::ExecCommandEnd(end)] = ends.as_slice() else {
        panic!("expected real end")
    };
    assert_eq!(end.status, ExecCommandStatus::Completed);
    assert_eq!(end.aggregated_output, "edited");
    assert_eq!(completed["observed"]["files"][0]["change"], "unchanged");
    assert_eq!(completed["strictRows"], serde_json::json!([]));
    assert!(
        completed["observed"]["files"][0]
            .get("operationIds")
            .is_none()
    );
    Ok(())
}

#[test]
fn observed_bytes_remain_separate_from_failed_process_and_strict_fallback() -> Result<()> {
    let root = tempfile::tempdir()?;
    let launch = Launch {
        call_id: "observation-fixture".into(),
        command: vec!["/bin/bash".into(), "-c".into(), "unknown_call".into()],
        cwd: root.path().to_string_lossy().into_owned(),
        started_at_ms: 10,
    };
    let (prepared, _) = flow::prepare(launch)?;
    let mut file = tempfile::NamedTempFile::new_in(root.path())?;
    file.write_all(b"authored test change\n")?;
    file.persist(root.path().join("settings.txt"))
        .map_err(|error| error.error)?;
    let completed = prepared.complete(Completion {
        exit_code: 1,
        output: "failed".into(),
        duration_micros: 100,
        completed_at_ms: 11,
    })?;
    let ends: Vec<EventMsg> = serde_json::from_value(completed["legacyEvents"].clone())?;
    let [EventMsg::ExecCommandEnd(end)] = ends.as_slice() else {
        panic!("expected end")
    };
    assert_eq!(end.status, ExecCommandStatus::Failed);
    assert_eq!(completed["observed"]["files"][0]["change"], "created");
    assert_eq!(completed["observed"]["basis"], "captureInterval");
    assert_eq!(completed["strictRows"], serde_json::json!([]));
    assert!(completed.get("report").is_none());
    Ok(())
}

#[test]
fn framing_and_completion_validation_reject_missing_or_unbounded_input() -> Result<()> {
    assert!(input::read_line(&mut Cursor::new(b"{}")).is_err());
    assert!(input::read_line(&mut Cursor::new(vec![b'x'; 262145])).is_err());
    assert_eq!(input::read_line(&mut Cursor::new(b""))?, None);
    let completion = Completion {
        exit_code: 0,
        output: String::new(),
        duration_micros: 1,
        completed_at_ms: 9,
    };
    assert!(completion.validate(10).is_err());
    assert!(
        serde_json::from_str::<Completion>(
            r#"{"exitCode":0,"output":"","durationMicros":1,"completedAtMs":10,"trusted":true}"#
        )
        .is_err()
    );
    Ok(())
}
