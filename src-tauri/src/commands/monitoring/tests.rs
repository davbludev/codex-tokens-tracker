use super::*;
use crate::source;
use std::{fs, io::Write, path::Path};

fn record(sequence: i64) -> String {
    let mut value: serde_json::Value = serde_json::from_str(
        include_str!("../../../../fixtures/codex/active-root.jsonl")
            .lines()
            .last()
            .unwrap(),
    )
    .unwrap();
    value["timestamp"] = format!("2026-01-01T00:{:02}:{:02}Z", sequence / 60, sequence % 60).into();
    value["payload"]["response_id"] = format!("monitoring-{sequence}").into();
    for count in value["payload"]["thread_token_usage"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        *count = (count.as_i64().unwrap() * sequence).into();
    }
    format!("{value}\n")
}

fn append(path: &Path, sequence: i64) {
    fs::OpenOptions::new()
        .append(true)
        .open(path)
        .unwrap()
        .write_all(record(sequence).as_bytes())
        .unwrap();
}

fn drain(runtime: &Runtime, work: &mut Work, store: &mut Store) {
    for _ in 0..20000 {
        if !runtime.step(work, store, Instant::now()).unwrap() && !work.busy() {
            return;
        }
    }
    panic!("monitoring did not become idle");
}

#[test]
fn pause_retains_atomic_checkpoint_and_resume_recovers_missed_appends_once() {
    let temp = tempfile::tempdir().unwrap();
    let sessions = temp.path().join("sessions");
    fs::create_dir(&sessions).unwrap();
    let path = source::normalized_path(&sessions.join("rollout-history.jsonl"));
    fs::write(&path, (1..=180).map(record).collect::<String>()).unwrap();
    let mut store = Store::open(&temp.path().join("usage.sqlite")).unwrap();
    let runtime = Runtime::default();
    let (control, _receive) = pricing::channel();
    let mut work = Work::new(temp.path());
    while work.batches == 0 {
        runtime.step(&mut work, &mut store, Instant::now()).unwrap();
    }
    let checkpoint = store.source_state(&path.to_string_lossy()).unwrap();
    assert_eq!(checkpoint.progress.ordinal, 64);
    runtime.set_paused(true, &control);
    append(&path, 181);
    for _ in 0..20 {
        runtime.step(&mut work, &mut store, Instant::now()).unwrap();
    }
    assert_eq!(
        store.source_state(&path.to_string_lossy()).unwrap(),
        checkpoint
    );
    assert!(!work.busy());
    assert_eq!(work.deadline(), None);
    runtime.set_paused(false, &control);
    drain(&runtime, &mut work, &mut store);
    assert_eq!(
        store
            .source_state(&path.to_string_lossy())
            .unwrap()
            .progress
            .ordinal,
        181
    );
    assert_eq!(
        store.snapshot().unwrap().direct_tokens,
        Some((181 * 26587).to_string())
    );

    // An append to a fully imported file must also be found without a watcher event.
    runtime.set_paused(true, &control);
    append(&path, 182);
    runtime.set_paused(false, &control);
    drain(&runtime, &mut work, &mut store);
    assert_eq!(
        store.snapshot().unwrap().direct_tokens,
        Some((182 * 26587).to_string())
    );
    assert_eq!(
        store
            .source_state(&path.to_string_lossy())
            .unwrap()
            .progress
            .ordinal,
        182
    );
}

#[test]
fn exit_stops_after_committed_batch_and_restart_resumes_without_draining_backlog() {
    let temp = tempfile::tempdir().unwrap();
    let sessions = temp.path().join("sessions");
    fs::create_dir(&sessions).unwrap();
    let path = source::normalized_path(&sessions.join("rollout-history.jsonl"));
    fs::write(&path, (1..=180).map(record).collect::<String>()).unwrap();
    let database = temp.path().join("usage.sqlite");
    let mut store = Store::open(&database).unwrap();
    let runtime = Runtime::default();
    let (control, receive) = pricing::channel();
    let mut work = Work::new(temp.path());
    while work.batches == 0 {
        runtime.step(&mut work, &mut store, Instant::now()).unwrap();
    }
    let committed = store.source_state(&path.to_string_lossy()).unwrap();
    assert_eq!(committed.progress.ordinal, 64);
    // The request remains nonblocking when source events saturate the shared inbox.
    for _ in 0..256 {
        control.0.try_send(pricing::Message::Wake).unwrap();
    }
    runtime.request_exit(&control);
    assert!(runtime.is_stopping());
    assert!(!runtime.is_finished());
    for _ in 0..20 {
        runtime.step(&mut work, &mut store, Instant::now()).unwrap();
    }
    assert_eq!(
        store.source_state(&path.to_string_lossy()).unwrap(),
        committed
    );
    store.close().unwrap();
    runtime.mark_finished();
    assert!(runtime.is_finished());
    assert_eq!(receive.try_iter().count(), 256);

    append(&path, 181);
    let mut store = Store::open(&database).unwrap();
    assert_eq!(
        store.source_state(&path.to_string_lossy()).unwrap(),
        committed
    );
    let restarted = Runtime::default();
    assert!(!restarted.is_paused());
    let mut work = Work::new(temp.path());
    drain(&restarted, &mut work, &mut store);
    assert_eq!(
        store.snapshot().unwrap().direct_tokens,
        Some((181 * 26587).to_string())
    );
    assert_eq!(
        store
            .source_state(&path.to_string_lossy())
            .unwrap()
            .progress
            .ordinal,
        181
    );
    let (idle_control, idle_inbox) = pricing::channel();
    restarted.request_exit(&idle_control);
    assert!(matches!(
        idle_inbox
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap(),
        pricing::Message::Wake
    ));
}

#[test]
fn source_settings_switch_keeps_pause_until_a_fresh_app_runtime_starts() {
    use crate::settings::{Config, Error};
    struct SourceOnlyPlatform;
    impl crate::desktop::Platform for SourceOnlyPlatform {
        fn autostart_enabled(&self) -> std::result::Result<bool, Error> {
            Ok(false)
        }
        fn tray_enabled(&self) -> bool {
            false
        }
        fn set_autostart(&self, _: bool) -> std::result::Result<(), Error> {
            panic!("source switch changed autostart")
        }
        fn set_tray(&self, _: bool) -> std::result::Result<(), Error> {
            panic!("source switch changed tray")
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let first = temp.path().join("first");
    let second = temp.path().join("second");
    fs::create_dir_all(first.join("sessions")).unwrap();
    fs::create_dir_all(second.join("sessions")).unwrap();
    fs::write(first.join("sessions/rollout-first.jsonl"), record(1)).unwrap();
    fs::write(second.join("sessions/rollout-second.jsonl"), record(2)).unwrap();
    let database = temp.path().join("usage.sqlite");
    let mut store = Store::open(&database).unwrap();
    let runtime = Runtime::default();
    let (control, _inbox) = pricing::channel();
    let mut work = Work::new(&first);
    drain(&runtime, &mut work, &mut store);
    runtime.set_paused(true, &control);
    let mut native = None;
    crate::commands::settings::apply(
        Config {
            codex_directory_override: Some(second.to_string_lossy().into_owned()),
            ..Default::default()
        },
        &mut store,
        &mut work,
        &mut native,
        &control,
        &SourceOnlyPlatform,
    )
    .unwrap();
    for _ in 0..20 {
        runtime.step(&mut work, &mut store, Instant::now()).unwrap();
    }
    assert!(runtime.is_paused());
    assert_eq!(
        store.snapshot().unwrap().direct_tokens.as_deref(),
        Some("26587")
    );
    assert_eq!(
        Store::read_diagnostics(&database)
            .unwrap()
            .usage_record_count,
        1
    );
    drop(native);
    store.close().unwrap();

    let mut store = Store::open(&database).unwrap();
    let saved = store
        .tracker_settings()
        .unwrap()
        .codex_directory_override
        .unwrap();
    let restarted = Runtime::default();
    assert!(!restarted.is_paused());
    let mut work = Work::new(Path::new(&saved));
    drain(&restarted, &mut work, &mut store);
    assert_eq!(
        store.snapshot().unwrap().direct_tokens,
        Some((2 * 26587).to_string())
    );
    assert_eq!(
        Store::read_diagnostics(&database)
            .unwrap()
            .usage_record_count,
        2
    );
}
