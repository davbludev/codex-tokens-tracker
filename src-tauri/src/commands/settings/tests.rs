use super::*;
use std::{
    fs,
    time::{Duration, Instant},
};

const ACTIVE: &str = include_str!("../../../../fixtures/codex/active-root.jsonl");

struct SourceOnlyPlatform;
impl crate::desktop::Platform for SourceOnlyPlatform {
    fn autostart_enabled(&self) -> Result<bool, Error> {
        Ok(false)
    }
    fn tray_enabled(&self) -> bool {
        false
    }
    fn set_autostart(&self, _: bool) -> Result<(), Error> {
        panic!("source-only settings changed autostart")
    }
    fn set_tray(&self, _: bool) -> Result<(), Error> {
        panic!("source-only settings changed the tray")
    }
}

fn apply(
    configuration: Config,
    store: &mut Store,
    work: &mut Work,
    native: &mut Option<NativeWatch>,
    control: &pricing::Control,
) -> Result<View, Error> {
    super::apply(
        configuration,
        store,
        work,
        native,
        control,
        &SourceOnlyPlatform,
    )
}

fn drain(work: &mut Work, store: &mut Store) {
    for _ in 0..2000 {
        if !work
            .step(store, Instant::now() + Duration::from_secs(1))
            .unwrap()
            && !work.busy()
        {
            return;
        }
    }
    panic!("source work did not become idle");
}

fn configuration(home: &Path) -> Config {
    Config {
        codex_directory_override: Some(home.to_string_lossy().into_owned()),
        ..Default::default()
    }
}

#[test]
fn settings_directory_switch_retains_history_deduplicates_and_watches_new_home() {
    let temp = tempfile::tempdir().unwrap();
    let first = temp.path().join("first");
    let second = temp.path().join("second");
    fs::create_dir_all(first.join("sessions")).unwrap();
    fs::create_dir_all(second.join("sessions")).unwrap();
    fs::write(first.join("sessions/rollout-original.jsonl"), ACTIVE).unwrap();
    fs::write(second.join("sessions/rollout-copy.jsonl"), ACTIVE).unwrap();
    let database = temp.path().join("usage.sqlite");
    let mut store = Store::open(&database).unwrap();
    let (control, inbox) = pricing::channel();
    let mut native = None;
    let mut work = Work::without_sources();
    apply(
        configuration(&first),
        &mut store,
        &mut work,
        &mut native,
        &control,
    )
    .unwrap();
    drain(&mut work, &mut store);
    assert_eq!(
        store.snapshot().unwrap().direct_tokens.as_deref(),
        Some("26587")
    );
    apply(
        configuration(&second),
        &mut store,
        &mut work,
        &mut native,
        &control,
    )
    .unwrap();
    drain(&mut work, &mut store);
    assert_eq!(
        Store::read_diagnostics(&database)
            .unwrap()
            .usage_record_count,
        1
    );
    assert_eq!(
        store.snapshot().unwrap().direct_tokens.as_deref(),
        Some("26587")
    );
    // Observe an actual native event from the replacement subscription.
    fs::write(
        second.join("sessions/rollout-new.jsonl"),
        ACTIVE.replace("root-active", "second-session"),
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while Store::read_diagnostics(&database)
        .unwrap()
        .tracked_session_count
        != 2
    {
        let pricing::Message::Source(event) = inbox
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap()
        else {
            panic!("expected native source event");
        };
        native
            .as_mut()
            .unwrap()
            .accept(&mut work, event, Instant::now());
        drain(&mut work, &mut store);
    }
    apply(
        configuration(&first),
        &mut store,
        &mut work,
        &mut native,
        &control,
    )
    .unwrap();
    drain(&mut work, &mut store);
    drop(store);
    let mut store = Store::open(&database).unwrap();
    let saved = store.tracker_settings().unwrap();
    assert_eq!(
        saved.codex_directory_override,
        Some(settings::validate_override(first.to_str().unwrap()).unwrap())
    );
    apply(saved, &mut store, &mut work, &mut native, &control).unwrap();
    drain(&mut work, &mut store);
    let diagnostics = Store::read_diagnostics(&database).unwrap();
    assert_eq!(diagnostics.tracked_session_count, 2);
    assert_eq!(diagnostics.usage_record_count, 2);
    assert!(diagnostics.last_successful_ingestion_at_ms.is_some());
}

#[test]
fn settings_failed_switch_leaves_saved_directory_and_active_work_intact() {
    let temp = tempfile::tempdir().unwrap();
    let first = temp.path().join("first");
    let second = temp.path().join("second");
    fs::create_dir_all(first.join("sessions")).unwrap();
    fs::create_dir_all(second.join("sessions")).unwrap();
    let mut store = Store::open(&temp.path().join("usage.sqlite")).unwrap();
    let (control, _inbox) = pricing::channel();
    let mut native = None;
    let mut work = Work::without_sources();
    apply(
        configuration(&first),
        &mut store,
        &mut work,
        &mut native,
        &control,
    )
    .unwrap();
    let roots = work.roots.clone();
    let saved = store.tracker_settings().unwrap().codex_directory_override;
    let invalid = apply(
        configuration(&temp.path().join("missing")),
        &mut store,
        &mut work,
        &mut native,
        &control,
    )
    .unwrap_err();
    assert_eq!(invalid.code, "invalid_directory");
    assert_eq!(work.roots, roots);
    assert_eq!(
        store.tracker_settings().unwrap().codex_directory_override,
        saved
    );
    // Inject a durable-write failure after the new watcher has been prepared.
    store
        .connection()
        .execute_batch("PRAGMA query_only=ON")
        .unwrap();
    assert_eq!(
        apply(
            configuration(&second),
            &mut store,
            &mut work,
            &mut native,
            &control
        )
        .unwrap_err()
        .code,
        "storage"
    );
    store
        .connection()
        .execute_batch("PRAGMA query_only=OFF")
        .unwrap();
    assert_eq!(work.roots, roots);
    assert_eq!(
        store.tracker_settings().unwrap().codex_directory_override,
        saved
    );
    fs::write(first.join("sessions/rollout-after-failure.jsonl"), ACTIVE).unwrap();
    drain(&mut work, &mut store);
    assert_eq!(
        store.snapshot().unwrap().direct_tokens.as_deref(),
        Some("26587")
    );
    assert!(native.is_some());
}

#[test]
fn shutdown_waits_for_csv_completion_and_rejects_new_exports() {
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("usage.sqlite");
    Store::open(&database).unwrap().close().unwrap();
    let destination = temp.path().join("sessions.csv");
    let exports = ExportState::default();
    let permit = exports.begin().unwrap();
    let (release, proceed) = mpsc::channel();
    let worker_destination = destination.clone();
    let worker = std::thread::spawn(move || {
        let _permit = permit;
        proceed.recv_timeout(Duration::from_secs(5)).unwrap();
        crate::export::export_csv(
            &database,
            crate::export::Request {
                kind: crate::export::Kind::Sessions,
                destination: worker_destination.to_string_lossy().into_owned(),
            },
        )
        .unwrap()
    });

    exports.begin_shutdown();
    assert!(!exports.is_idle());
    assert!(matches!(exports.begin(), Err(crate::export::Error::Busy)));
    let waiting = exports.clone();
    let (finished, completion) = mpsc::channel();
    let waiter = std::thread::spawn(move || {
        waiting.wait_for_idle();
        finished.send(()).unwrap();
    });
    // The export is deliberately held before its write; shutdown must stay pending.
    assert!(matches!(
        completion.recv_timeout(Duration::from_millis(100)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    release.send(()).unwrap();
    completion.recv_timeout(Duration::from_secs(5)).unwrap();
    waiter.join().unwrap();
    assert_eq!(worker.join().unwrap().row_count, 0);
    let csv = fs::read_to_string(destination).unwrap();
    assert!(csv.starts_with("session_id,"));
    assert!(csv.ends_with("estimated_cost_state,coverage_note\r\n"));
    assert!(exports.is_idle());
    assert!(matches!(exports.begin(), Err(crate::export::Error::Busy)));
}
