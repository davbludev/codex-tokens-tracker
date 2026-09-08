use crate::{adapter, source, storage::Store};
use std::{fs, io::Write};
mod aggregates;
mod pricing;
mod weekly;

const ACTIVE: &str = include_str!("../../fixtures/codex/active-root.jsonl");
const CHILD: &str = include_str!("../../fixtures/codex/completed-child.jsonl");
const CONFLICT: &str = include_str!("../../fixtures/codex/scope-conflict.jsonl");

fn append(path: &std::path::Path, value: &str) {
    fs::OpenOptions::new()
        .append(true)
        .open(path)
        .unwrap()
        .write_all(value.as_bytes())
        .unwrap();
}

#[test]
fn recovery_identity_initialization_is_guarded_without_generation_changes() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("identity.sqlite")).unwrap();
    store.source_state("new").unwrap();
    assert!(store
        .initialize_source_identity("new", 1, "id", 10)
        .is_err());
    store
        .initialize_source_identity("new", 0, "id", 10)
        .unwrap();
    let bound = store.source_state("new").unwrap();
    assert_eq!(bound.generation, 0);
    assert_eq!(bound.progress.offset, 0);
    assert_eq!(bound.identity.as_deref(), Some("id"));
    assert!(store
        .initialize_source_identity("new", 0, "different", 10)
        .is_err());
    record_in_store(&mut store, "used", &historical_record(1));
    assert!(store
        .initialize_source_identity("used", 0, "id", 10)
        .is_err());
    assert_eq!(totals(&store), (26587, 0, 1));
}

#[test]
fn recovery_snapshot_uses_parsed_chronology_and_reports_unresolved_usage() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("snapshot.sqlite")).unwrap();
    let mut newer = historical_record(1);
    newer["payload"]["thread_id"] = "newer".into();
    newer["timestamp"] = "2026-01-02T00:00:00Z".into();
    record_in_store(&mut store, "live", &newer);
    let mut older = historical_record(1);
    older["payload"]["thread_id"] = "older".into();
    older["timestamp"] = "2026-01-02T01:00:00+02:00".into();
    record_in_store(&mut store, "imported", &older);
    assert_eq!(
        store.snapshot().unwrap().thread_id.as_deref(),
        Some("newer")
    );
    let mut tied = historical_record(1);
    tied["payload"]["thread_id"] = "aaa-tie".into();
    tied["timestamp"] = "2026-01-02T02:00:00+02:00".into();
    record_in_store(&mut store, "tied", &tied);
    assert_eq!(
        store.snapshot().unwrap().thread_id.as_deref(),
        Some("aaa-tie")
    );
    let mut invalid = historical_record(1);
    invalid["payload"]["thread_id"] = "invalid-time".into();
    invalid["timestamp"] = "not-a-timestamp".into();
    record_in_store(&mut store, "invalid", &invalid);
    assert_eq!(
        store.snapshot().unwrap().thread_id.as_deref(),
        Some("aaa-tie")
    );

    let mut gap = historical_record(3);
    gap["payload"]["thread_id"] = "aaa-tie".into();
    gap["timestamp"] = "2026-01-02T03:00:00Z".into();
    record_in_store(&mut store, "gap", &gap);
    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot.direct_tokens.as_deref(), Some("26587"));
    assert!(snapshot.coverage.contains("Incomplete"));
    assert!(snapshot.diagnostic.unwrap().contains("pending"));
    assert_eq!(
        snapshot.observed_at.as_deref(),
        Some("2026-01-02T03:00:00Z")
    );
    // Persisted pending-only states may exist at the resumable promotion boundary.
    store.connection().execute("UPDATE observations SET accepted=0,total=NULL,state='pending' WHERE thread_id='aaa-tie'", []).unwrap();
    let pending = store.snapshot().unwrap();
    assert_eq!(pending.direct_tokens, None);
    assert!(pending.coverage.contains("unavailable"));
}

#[test]
fn recovery_v2_migration_and_snapshot_query_plan() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("v2.sqlite");
    let connection = rusqlite::Connection::open(&db).unwrap();
    connection
        .execute_batch(include_str!("../migrations/001_initial.sql"))
        .unwrap();
    connection
        .execute_batch(include_str!("../migrations/002_resumable_sources.sql"))
        .unwrap();
    drop(connection);
    let mut store = Store::open(&db).unwrap();
    for sequence in 1..=1000 {
        record_in_store(&mut store, "representative", &historical_record(sequence));
    }
    let query = "SELECT thread_id,timestamp FROM observations WHERE time_seconds IS NOT NULL AND time_nanos IS NOT NULL ORDER BY time_seconds DESC,time_nanos DESC,thread_id ASC LIMIT 1";
    let plan: Vec<String> = store
        .connection()
        .prepare(&format!("EXPLAIN QUERY PLAN {query}"))
        .unwrap()
        .query_map([], |row| row.get(3))
        .unwrap()
        .collect::<std::result::Result<_, _>>()
        .unwrap();
    assert!(
        plan.iter().any(|line| line.contains("observation_latest")),
        "{plan:?}"
    );
    assert!(
        !plan.iter().any(|line| line.contains("TEMP B-TREE")),
        "{plan:?}"
    );
    let sum_plan: Vec<String> = store.connection().prepare("EXPLAIN QUERY PLAN SELECT SUM(total) FROM observations WHERE thread_id='root-active' AND accepted=1").unwrap().query_map([], |row| row.get(3)).unwrap().collect::<std::result::Result<_,_>>().unwrap();
    assert!(
        sum_plan
            .iter()
            .any(|line| line.contains("observation_session")),
        "{sum_plan:?}"
    );
    let started = std::time::Instant::now();
    let snapshot = store.snapshot().unwrap();
    eprintln!(
        "1,000-observation snapshot: {:?}; latest plan: {plan:?}; sum plan: {sum_plan:?}",
        started.elapsed()
    );
    assert_eq!(snapshot.direct_tokens.as_deref(), Some("26587000"));
    drop(store);
    let store = Store::open(&db).unwrap();
    assert_eq!(
        store
            .connection()
            .query_row::<i64, _, _>("PRAGMA user_version", [], |r| r.get(0))
            .unwrap(),
        9
    );
    assert_eq!(
        store.snapshot().unwrap().direct_tokens.as_deref(),
        Some("26587000")
    );
}

#[test]
fn recovery_same_size_rewrite_replacement_truncation_and_archive_retain_usage() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("recovery.sqlite");
    let path = temp.path().join("rollout-live.jsonl");
    let mut store = Store::open(&db).unwrap();
    let one = line(&historical_record(1));
    let two = line(&historical_record(2));
    assert_eq!(one.len(), two.len());
    fs::write(&path, &one).unwrap();
    source::ingest(&mut store, &path).unwrap();
    let key = path.to_string_lossy();
    let original = store.source_state(&key).unwrap();
    assert_eq!(original.generation, 0);
    // Rewriting the same inode at the same size is detected by its fingerprint.
    fs::write(&path, &two).unwrap();
    source::ingest(&mut store, &path).unwrap();
    assert_eq!(store.source_state(&key).unwrap().generation, 1);
    assert_eq!(totals(&store), (2 * 26587, 0, 2));
    let replacement = temp.path().join("replacement");
    fs::write(&replacement, &two).unwrap();
    fs::remove_file(&path).unwrap();
    fs::rename(&replacement, &path).unwrap();
    source::ingest(&mut store, &path).unwrap();
    assert_eq!(store.source_state(&key).unwrap().generation, 2);
    fs::write(&path, "").unwrap();
    source::ingest(&mut store, &path).unwrap();
    assert_eq!(store.source_state(&key).unwrap().generation, 3);
    append(&path, &line(&historical_record(3)));
    source::ingest(&mut store, &path).unwrap();
    let archived = temp.path().join("rollout-archive.jsonl");
    fs::rename(&path, &archived).unwrap();
    source::ingest(&mut store, &path).unwrap();
    source::ingest(&mut store, &archived).unwrap();
    drop(store);
    let mut store = Store::open(&db).unwrap();
    source::ingest(&mut store, &archived).unwrap();
    assert_eq!(totals(&store), (3 * 26587, 0, 3));
}

#[test]
fn recovery_reader_bounds_oversized_tails_and_resumes_partial_after_restart() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("tails.sqlite");
    let path = temp.path().join("rollout-tail.jsonl");
    let mut store = Store::open(&db).unwrap();
    let record = line(&historical_record(1));
    fs::write(&path, &record[..record.len() / 2]).unwrap();
    assert!(!source::ingest_batch(&mut store, &path).unwrap());
    let state = store.source_state(&path.to_string_lossy()).unwrap();
    assert_eq!(state.progress.offset, 0);
    assert_eq!(state.progress.tail_length, (record.len() / 2) as u64);
    assert!(state.progress.verification_hash.is_some());
    drop(store);
    append(&path, &record[record.len() / 2..]);
    let mut store = Store::open(&db).unwrap();
    source::ingest(&mut store, &path).unwrap();
    assert_eq!(totals(&store), (26587, 0, 1));

    let oversized = temp.path().join("rollout-oversized.jsonl");
    fs::write(&oversized, vec![b'x'; 3 * 1024 * 1024]).unwrap();
    let mut steps = 0;
    loop {
        steps += 1;
        let more = source::ingest_batch(&mut store, &oversized).unwrap();
        let state = store.source_state(&oversized.to_string_lossy()).unwrap();
        assert!(state.progress.tail_length <= steps * 256 * 1024);
        if !more {
            break;
        }
        assert!(steps < 20);
    }
    assert_eq!(steps, 12);
    let before = store.source_state(&oversized.to_string_lossy()).unwrap();
    assert!(before.progress.tail_discarding);
    assert_eq!(before.progress.offset, 0);
    drop(store);
    append(&oversized, "\n");
    let mut store = Store::open(&db).unwrap();
    source::ingest(&mut store, &oversized).unwrap();
    let after = store.source_state(&oversized.to_string_lossy()).unwrap();
    assert_eq!(after.progress.ordinal, 1);
    assert_eq!(after.progress.tail_length, 0);
    assert_eq!(after.progress.offset, 3 * 1024 * 1024 + 1);
    assert_eq!(totals(&store), (26587, 0, 1));
}

#[test]
fn recovery_interrupted_import_resumes_from_atomic_batch_checkpoint() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("interrupted.sqlite");
    let path = temp.path().join("rollout-history.jsonl");
    let records: String = (1..=180)
        .map(|sequence| line(&historical_record(sequence)))
        .collect();
    fs::write(&path, &records).unwrap();
    let mut store = Store::open(&db).unwrap();
    assert!(source::ingest_batch(&mut store, &path).unwrap());
    let committed = store.source_state(&path.to_string_lossy()).unwrap();
    assert_eq!(committed.progress.ordinal, 64);
    assert_eq!(totals(&store), (64 * 26587, 0, 64));
    drop(store);
    // A live append overlaps the persisted, unfinished historical import.
    append(&path, &line(&historical_record(181)));
    let mut store = Store::open(&db).unwrap();
    source::ingest(&mut store, &path).unwrap();
    assert_eq!(totals(&store), (181 * 26587, 0, 181));
    let final_state = store.source_state(&path.to_string_lossy()).unwrap();
    assert_eq!(final_state.generation, committed.generation);
    assert_eq!(final_state.progress.ordinal, 181);
    assert_eq!(
        final_state.progress.offset,
        fs::metadata(&path).unwrap().len()
    );
}

#[test]
fn recovery_source_sweep_is_keyset_bounded_resumable_and_scoped() {
    use crate::{commands::runtime::Work, storage::SourceProgress};
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("sweep.sqlite");
    let mut store = Store::open(&db).unwrap();
    let sessions = temp.path().join("sessions");
    let mut names = Vec::new();
    for number in 0..130 {
        let path = sessions
            .join(format!("rollout-{number:03}.jsonl"))
            .to_string_lossy()
            .into_owned();
        store.source_state(&path).unwrap();
        store
            .batch(
                &path,
                0,
                vec![],
                SourceProgress {
                    known_size: 16,
                    tail_length: 16,
                    ..Default::default()
                },
            )
            .unwrap();
        names.push(path);
    }
    let through = store.source_watermark().unwrap().unwrap();
    let later = sessions
        .join("rollout-zzz.jsonl")
        .to_string_lossy()
        .into_owned();
    store.source_state(&later).unwrap();
    let mut after = None;
    let mut collected = Vec::new();
    loop {
        let page = store.source_page(after.as_deref(), &through).unwrap();
        assert!(page.len() <= 64);
        if page.is_empty() {
            break;
        }
        after = page.last().map(|(path, _)| path.clone());
        collected.extend(page.into_iter().map(|(path, _)| path));
    }
    assert_eq!(collected, names);
    store.source_state("outside-selected-home").unwrap();
    store
        .batch(
            "outside-selected-home",
            0,
            vec![],
            SourceProgress {
                known_size: 10,
                tail_length: 10,
                ..Default::default()
            },
        )
        .unwrap();
    let mut work = Work::new(temp.path());
    let now = std::time::Instant::now();
    for _ in 0..3 {
        work.step(&mut store, now).unwrap();
    }
    let removed: i64 = store
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM sources WHERE diagnostic LIKE 'Source removed%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(removed, 64);
    drop(work);
    drop(store);
    // Restart after only the first page: the pass safely begins again.
    let mut store = Store::open(&db).unwrap();
    let mut work = Work::new(temp.path());
    drain_work(&mut work, &mut store, now);
    let remaining: i64 = store
        .connection()
        .query_row("SELECT COUNT(*) FROM sources WHERE partial=1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(remaining, 1); // The unrelated home is never inspected or marked removed.
    for path in names {
        assert_eq!(store.source_state(&path).unwrap().progress.tail_length, 0);
    }
    assert!(!work.busy());
}

#[test]
fn recovery_directory_rename_removal_and_return_preserve_confirmed_usage() {
    use crate::commands::runtime::Work;
    use notify::{
        event::{ModifyKind, RemoveKind, RenameMode},
        Event, EventKind,
    };
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join("sessions").join("nested");
    let archived = temp.path().join("archived_sessions");
    fs::create_dir_all(&directory).unwrap();
    fs::create_dir(&archived).unwrap();
    let path = directory.join("rollout-normal.jsonl");
    let one = line(&historical_record(1));
    let two = line(&historical_record(2));
    fs::write(&path, format!("{one}{}", &two[..two.len() / 2])).unwrap();
    let large = directory.join("rollout-large.jsonl");
    fs::write(&large, vec![b'x'; 3 * 1024 * 1024]).unwrap();
    let mut store = Store::open(&temp.path().join("remove.sqlite")).unwrap();
    let mut work = Work::new(temp.path());
    let now = std::time::Instant::now();
    drain_work(&mut work, &mut store, now);
    assert_eq!(totals(&store), (26587, 0, 1));
    assert!(
        store
            .source_state(&large.to_string_lossy())
            .unwrap()
            .progress
            .tail_discarding
    );
    let renamed = archived.join("nested");
    fs::rename(&directory, &renamed).unwrap();
    work.event(
        Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
            .add_path(directory.clone())
            .add_path(renamed.clone()),
        now,
    );
    drain_work(&mut work, &mut store, now);
    assert_eq!(
        store
            .source_state(&path.to_string_lossy())
            .unwrap()
            .progress
            .tail_length,
        0
    );
    assert_eq!(
        store
            .source_state(&large.to_string_lossy())
            .unwrap()
            .progress
            .tail_length,
        0
    );
    assert_eq!(totals(&store), (26587, 0, 1));
    fs::remove_dir_all(&renamed).unwrap();
    work.event(
        Event::new(EventKind::Remove(RemoveKind::Folder)).add_path(renamed),
        now,
    );
    drain_work(&mut work, &mut store, now);
    let partial: bool = store
        .connection()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sources WHERE partial=1)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!partial);
    fs::create_dir_all(&directory).unwrap();
    fs::write(&path, format!("{one}{two}")).unwrap();
    work.recover("Source returned; recovering available files");
    drain_work(&mut work, &mut store, now);
    assert_eq!(totals(&store), (2 * 26587, 0, 2));
    append(&path, &line(&historical_record(3)));
    source::ingest(&mut store, &path).unwrap();
    assert_eq!(totals(&store), (3 * 26587, 0, 3));
}

#[test]
fn recovery_permission_or_sharing_failure_never_marks_source_removed() {
    use crate::storage::SourceProgress;
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("permission.sqlite")).unwrap();
    store.source_state("source").unwrap();
    store
        .batch(
            "source",
            0,
            vec![],
            SourceProgress {
                known_size: 32,
                tail_length: 32,
                verification_length: 32,
                verification_hash: Some([1; 32]),
                ..Default::default()
            },
        )
        .unwrap();
    let before = store.source_state("source").unwrap();
    for kind in [
        std::io::ErrorKind::PermissionDenied,
        std::io::ErrorKind::Other,
    ] {
        assert!(
            source::reconcile_presence(&store, "source", 0, Err(std::io::Error::from(kind)))
                .is_err()
        );
        assert_eq!(store.source_state("source").unwrap(), before);
    }
    source::reconcile_presence(
        &store,
        "source",
        0,
        Err(std::io::Error::from(std::io::ErrorKind::NotFound)),
    )
    .unwrap();
    assert_eq!(
        store.source_state("source").unwrap().progress.tail_length,
        0
    );
}

fn drain_work(
    work: &mut crate::commands::runtime::Work,
    store: &mut Store,
    now: std::time::Instant,
) {
    for _ in 0..20000 {
        if !work.step(store, now).unwrap() && !work.busy() {
            return;
        }
    }
    panic!("bounded work did not become idle");
}

#[test]
fn recovery_import_live_overlap_debounce_fairness_and_idle() {
    use crate::commands::runtime::{Work, DEBOUNCE};
    use notify::{event::ModifyKind, Event, EventKind};
    let temp = tempfile::tempdir().unwrap();
    let sessions = temp.path().join("sessions");
    let archive = temp.path().join("archived_sessions");
    fs::create_dir_all(&sessions).unwrap();
    fs::create_dir_all(&archive).unwrap();
    let history = sessions.join("rollout-history.jsonl");
    let ignored = "{\"type\":\"response_item\",\"payload\":{}}\n";
    fs::write(&history, ignored.repeat(10000)).unwrap();
    let live = sessions.join("rollout-live.jsonl");
    fs::write(&live, line(&historical_record(2))).unwrap();
    fs::write(
        archive.join("rollout-old.jsonl"),
        line(&historical_record(1)),
    )
    .unwrap();
    fs::write(archive.join("unrelated.jsonl"), line(&historical_record(4))).unwrap();
    let mut store = Store::open(&temp.path().join("runtime.sqlite")).unwrap();
    let mut work = Work::new(temp.path());
    let now = std::time::Instant::now();
    // Trigger a live append while the large historical file is still importing.
    for _ in 0..5 {
        work.step(&mut store, now).unwrap();
    }
    append(&live, &line(&historical_record(3)));
    for _ in 0..100 {
        work.event(
            Event::new(EventKind::Modify(ModifyKind::Any)).add_path(live.clone()),
            now,
        );
    }
    assert_eq!(work.deadline(), Some(now + DEBOUNCE));
    for _ in 0..30 {
        work.step(&mut store, now + DEBOUNCE).unwrap();
    }
    assert_eq!(totals(&store), (3 * 26587, 0, 3));
    assert!(
        store.checkpoint(&history.to_string_lossy()).unwrap().0
            < fs::metadata(&history).unwrap().len()
    );
    drain_work(&mut work, &mut store, now + DEBOUNCE);
    assert_eq!(work.discovered, 3);
    assert!(work.progress().contains("Live monitoring"));
    assert!(work.progress().len() < 256);
    let changes = store.connection().total_changes();
    let batches = work.batches;
    for _ in 0..10 {
        assert!(!work.step(&mut store, now + DEBOUNCE).unwrap());
    }
    assert_eq!(store.connection().total_changes(), changes);
    assert_eq!(work.batches, batches);
    assert_eq!(work.deadline(), None);
}

#[test]
fn recovery_overflow_errors_and_directory_rename_discover_missed_sources() {
    use crate::commands::runtime::Work;
    use notify::{
        event::{ModifyKind, RenameMode},
        Event, EventKind,
    };
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("sessions")).unwrap();
    let mut store = Store::open(&temp.path().join("events.sqlite")).unwrap();
    let mut work = Work::new(temp.path());
    let now = std::time::Instant::now();
    drain_work(&mut work, &mut store, now);
    let path = temp.path().join("sessions/rollout-missed.jsonl");
    fs::write(&path, line(&historical_record(1))).unwrap();
    work.recover("Source events overflowed; recovering available files");
    drain_work(&mut work, &mut store, now);
    assert_eq!(totals(&store), (26587, 0, 1));
    append(&path, &line(&historical_record(2)));
    work.recover("Native watcher reported an error; recovering available files");
    drain_work(&mut work, &mut store, now);
    assert_eq!(totals(&store), (2 * 26587, 0, 2));
    let directory = temp.path().join("sessions/moved");
    fs::create_dir(&directory).unwrap();
    fs::write(
        directory.join("rollout-new.jsonl"),
        line(&historical_record(3)),
    )
    .unwrap();
    work.event(
        Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::To))).add_path(directory),
        now,
    );
    drain_work(&mut work, &mut store, now);
    assert_eq!(totals(&store), (3 * 26587, 0, 3));
}

#[test]
fn recovery_native_missing_home_and_sources_appear_without_restart() {
    use crate::commands::{runtime::Work, NativeWatch};
    use std::time::{Duration, Instant};
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("new-home");
    let mut native = NativeWatch::new(&home).unwrap();
    let mut work = Work::new(&home);
    let mut store = Store::open(&temp.path().join("native.sqlite")).unwrap();
    drain_work(&mut work, &mut store, Instant::now());
    assert!(work.progress().contains("Waiting"));
    let sessions = home.join("sessions").join("2026").join("09").join("07");
    fs::create_dir_all(&sessions).unwrap();
    let path = sessions.join("rollout-native.jsonl");
    fs::write(&path, line(&historical_record(1))).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while totals(&store).0 != 26587 {
        let crate::commands::pricing::Message::Source(event) = native
            .receive
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap()
        else {
            panic!("expected source event")
        };
        native.accept(&mut work, event, Instant::now());
        drain_work(
            &mut work,
            &mut store,
            Instant::now() + Duration::from_secs(1),
        );
    }
    append(&path, &line(&historical_record(2)));
    while totals(&store).0 != 2 * 26587 {
        let crate::commands::pricing::Message::Source(event) = native
            .receive
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap()
        else {
            panic!("expected source event")
        };
        native.accept(&mut work, event, Instant::now());
        drain_work(
            &mut work,
            &mut store,
            Instant::now() + Duration::from_secs(1),
        );
    }
    assert_eq!(
        store.checkpoint(&path.to_string_lossy()).unwrap().0,
        fs::metadata(&path).unwrap().len()
    );
}

#[test]
fn recovery_native_queue_overflow_and_error_restore_missed_records() {
    use crate::commands::{runtime::Work, NativeWatch};
    use std::{
        sync::atomic::Ordering,
        time::{Duration, Instant},
    };
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("codex");
    let sessions = home.join("sessions");
    fs::create_dir_all(&sessions).unwrap();
    let mut native = NativeWatch::new(&home).unwrap();
    let mut work = Work::new(&home);
    let mut store = Store::open(&temp.path().join("overflow.sqlite")).unwrap();
    drain_work(&mut work, &mut store, Instant::now());
    // Deliberately stop consuming the production bounded channel during a burst.
    for number in 0..600 {
        fs::write(sessions.join(format!("rollout-{number}.jsonl")), "").unwrap();
    }
    let missed = sessions.join("rollout-missed.jsonl");
    fs::write(&missed, line(&historical_record(1))).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !native.overflow.load(Ordering::Relaxed) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(
        native.recover_overflow(&mut work),
        "native burst must exercise channel overflow"
    );
    drain_work(&mut work, &mut store, Instant::now());
    assert_eq!(totals(&store), (26587, 0, 1));
    append(&missed, &line(&historical_record(2)));
    native.accept(
        &mut work,
        Err(notify::Error::generic("synthetic watcher failure")),
        Instant::now(),
    );
    drain_work(&mut work, &mut store, Instant::now());
    assert_eq!(totals(&store), (2 * 26587, 0, 2));
    assert!(!work.busy());
}
fn modern() -> serde_json::Value {
    serde_json::from_str(ACTIVE.lines().last().unwrap()).unwrap()
}
fn line(value: &serde_json::Value) -> String {
    format!("{value}\n")
}

#[test]
fn fixture_usage_restart_replay_and_cross_file_identity() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("test.sqlite");
    let path = temp.path().join("rollout-a.jsonl");
    fs::write(&path, ACTIVE).unwrap();
    let mut store = Store::open(&db).unwrap();
    source::ingest(&mut store, &path).unwrap();
    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot.direct_tokens.as_deref(), Some("26587")); // 26119 input + 468 output.
    assert_eq!(snapshot.thread_id.as_deref(), Some("root-active"));
    drop(store);
    let mut store = Store::open(&db).unwrap();
    source::ingest(&mut store, &path).unwrap();
    append(&path, &line(&modern()));
    source::ingest(&mut store, &path).unwrap();
    let replay = temp.path().join("rollout-replay.jsonl");
    fs::write(&replay, ACTIVE).unwrap();
    source::ingest(&mut store, &replay).unwrap();
    assert_eq!(
        store.snapshot().unwrap().direct_tokens.as_deref(),
        Some("26587")
    );
    let count: i64 = store
        .connection()
        .query_row("SELECT COUNT(*) FROM observations", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
    let checkpoint = store.checkpoint(&path.to_string_lossy()).unwrap();
    assert_eq!(checkpoint.0, fs::metadata(&path).unwrap().len());
}

#[test]
fn opening_explicit_usage_legacy_mirror_and_gap_stop() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("test.sqlite")).unwrap();
    let path = temp.path().join("rollout-conflict.jsonl");
    fs::write(&path, CONFLICT).unwrap();
    source::ingest(&mut store, &path).unwrap();
    assert_eq!(
        store.snapshot().unwrap().direct_tokens.as_deref(),
        Some("53160")
    );
    assert_eq!(
        store.snapshot().unwrap().thread_id.as_deref(),
        Some("child-conflict")
    );
    let child = temp.path().join("rollout-child.jsonl");
    fs::write(&child, CHILD).unwrap();
    source::ingest(&mut store, &child).unwrap();
    // This excerpt omits the middle of the stream: stop after the first verified response.
    assert_eq!(
        store
            .connection()
            .query_row::<i64, _, _>(
                "SELECT SUM(total) FROM observations WHERE thread_id='child-a' AND accepted=1",
                [],
                |r| r.get(0)
            )
            .unwrap(),
        30444
    );
    // Importing an older session does not displace the latest source observation.
    assert_eq!(
        store.snapshot().unwrap().thread_id.as_deref(),
        Some("child-conflict")
    );
    assert!(store.snapshot().unwrap().diagnostic.is_some());
}

#[test]
fn incomplete_tail_completes_once_and_commits_atomically() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("test.sqlite")).unwrap();
    let path = temp.path().join("rollout-tail.jsonl");
    let split = ACTIVE.rfind("\"thread_token_usage\"").unwrap();
    fs::write(&path, &ACTIVE[..split]).unwrap();
    source::ingest(&mut store, &path).unwrap();
    assert_eq!(store.snapshot().unwrap().direct_tokens, None);
    let checkpoint = store.checkpoint(&path.to_string_lossy()).unwrap();
    assert!(checkpoint.0 < split as u64);
    append(&path, &ACTIVE[split..]);
    store.connection().execute_batch("CREATE TRIGGER fail_checkpoint BEFORE UPDATE OF offset ON sources BEGIN SELECT RAISE(ABORT, 'test interruption'); END;").unwrap();
    assert!(source::ingest(&mut store, &path).is_err());
    assert_eq!(
        store.checkpoint(&path.to_string_lossy()).unwrap(),
        checkpoint
    );
    assert_eq!(store.snapshot().unwrap().direct_tokens, None);
    store
        .connection()
        .execute_batch("DROP TRIGGER fail_checkpoint")
        .unwrap();
    source::ingest(&mut store, &path).unwrap();
    assert_eq!(
        store.snapshot().unwrap().direct_tokens.as_deref(),
        Some("26587")
    );
    source::ingest(&mut store, &path).unwrap();
    assert_eq!(
        store.snapshot().unwrap().direct_tokens.as_deref(),
        Some("26587")
    );
}

#[test]
fn conflicts_decreases_missing_categories_and_malformed_sources_are_unavailable() {
    for case in ["conflict", "decrease", "missing", "malformed"] {
        let temp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&temp.path().join("test.sqlite")).unwrap();
        let path = temp.path().join("rollout-bad.jsonl");
        fs::write(&path, ACTIVE).unwrap();
        source::ingest(&mut store, &path).unwrap();
        let mut record = modern();
        match case {
            "conflict" => record["payload"]["usage"]["total_tokens"] = 1.into(),
            "decrease" => {
                record["payload"]["response_id"] = "next".into();
                record["payload"]["thread_token_usage"]["total_tokens"] = 1.into();
            }
            "missing" => {
                record["payload"]["response_id"] = "next".into();
                record["payload"]["thread_token_usage"]["total_tokens"] = 53174.into();
                record["payload"]["usage"]
                    .as_object_mut()
                    .unwrap()
                    .remove("cached_input_tokens");
            }
            _ => (),
        }
        append(
            &path,
            if case == "malformed" {
                "{not-json}\n".into()
            } else {
                line(&record)
            }
            .as_str(),
        );
        source::ingest(&mut store, &path).unwrap();
        assert_eq!(
            store.snapshot().unwrap().direct_tokens.as_deref(),
            Some("26587")
        );
        assert!(store.snapshot().unwrap().diagnostic.is_some());
        // A separate valid direct session still imports after this source fails.
        let other = temp.path().join("rollout-other.jsonl");
        fs::write(&other, CONFLICT).unwrap();
        source::ingest(&mut store, &other).unwrap();
        assert_eq!(
            store.snapshot().unwrap().direct_tokens.as_deref(),
            Some("53160")
        );
    }
}

#[test]
fn limits_preserve_precision_buckets_time_and_conflicts_without_token_counting() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("test.sqlite")).unwrap();
    let path = temp.path().join("rollout-limits.jsonl");
    let sample = r#"{"timestamp":"2026-01-01T00:00:00Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"limit_id":"codex","primary":{"window_minutes":10080,"used_percent":1.2500,"resets_at":null},"secondary":{"window_minutes":300,"used_percent":2}}}}"#;
    fs::write(
        &path,
        format!(
            "{sample}\n{sample}\n{}\n{}\n{}\n",
            sample.replace("00:00:00", "00:00:01"),
            sample.replace("1.2500", "1.5000"),
            sample.replace("codex", "other")
        ),
    )
    .unwrap();
    source::ingest(&mut store, &path).unwrap();
    let count: i64 = store
        .connection()
        .query_row("SELECT COUNT(*) FROM limit_samples", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 7); // Two windows × three distinct time/bucket pairs, plus one conflict.
    let precision: String = store
        .connection()
        .query_row(
            "SELECT used_percent FROM limit_samples WHERE window_minutes=10080 ORDER BY id LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(precision, "1.2500");
    assert_eq!(store.snapshot().unwrap().direct_tokens, None);
    assert!(store
        .snapshot()
        .unwrap()
        .diagnostic
        .unwrap()
        .contains("Legacy-only"));
}

#[test]
fn adapter_drops_content_and_preserves_missing_versus_null_categories() {
    assert!(matches!(
        adapter::decode(br#"{"type":"response_item","payload":{"content":"DO NOT RETAIN"}}"#),
        Ok(adapter::Record::Ignore)
    ));
    let mut record = modern();
    record["payload"]["usage"]
        .as_object_mut()
        .unwrap()
        .remove("cached_input_tokens");
    record["payload"]["usage"]["reasoning_output_tokens"] = serde_json::Value::Null;
    let adapter::Record::Usage { usage, .. } = adapter::decode(line(&record).as_bytes()).unwrap()
    else {
        panic!("usage required")
    };
    let normalized = serde_json::to_value(usage).unwrap();
    assert!(normalized["usage"].get("cached_input_tokens").is_none());
    assert!(normalized["usage"]["reasoning_output_tokens"].is_null());
    let adapter::Record::Metadata(meta) = adapter::decode(br#"{"type":"session_meta","payload":{"id":"child","source":{"subagent":{"thread_spawn":{"parent_thread_id":"parent"}}}}}"#).unwrap() else { panic!("metadata required") };
    let metadata = serde_json::to_value(meta).unwrap();
    assert!(metadata["parent_thread_id"].is_null());
    assert_eq!(metadata["nested_parent_thread_id"], "parent");
    assert!(metadata.get("source").is_none());
}

#[test]
fn unknown_envelopes_do_not_suppress_reconciled_modern_usage() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("test.sqlite")).unwrap();
    let path = temp.path().join("rollout-unknown.jsonl");
    fs::write(
        &path,
        format!(
            "{{\"type\":\"world_state\",\"payload\":{{\"content\":\"DO NOT RETAIN\"}}}}\n{ACTIVE}"
        ),
    )
    .unwrap();
    source::ingest(&mut store, &path).unwrap();
    assert_eq!(
        store.snapshot().unwrap().direct_tokens.as_deref(),
        Some("26587")
    );
    assert!(store
        .snapshot()
        .unwrap()
        .diagnostic
        .unwrap()
        .contains("Unrecognized envelope"));
}

#[test]
fn native_append_accepts_only_the_new_reconciled_delta() {
    use notify::{RecursiveMode, Watcher};
    use std::time::{Duration, Instant};
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("test.sqlite")).unwrap();
    let path = temp.path().join("rollout-live.jsonl");
    fs::write(&path, ACTIVE).unwrap();
    source::ingest(&mut store, &path).unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |event| {
        let _ = send.send(event);
    })
    .unwrap();
    watcher
        .watch(temp.path(), RecursiveMode::NonRecursive)
        .unwrap();
    let mut record = modern();
    record["timestamp"] = "2026-01-01T11:34:00.000Z".into();
    record["payload"]["response_id"] = "second-response".into();
    for value in record["payload"]["thread_token_usage"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        *value = (value.as_i64().unwrap() * 2).into();
    }
    let started = Instant::now();
    append(&path, &line(&record));
    loop {
        let remaining = Duration::from_secs(5)
            .checked_sub(started.elapsed())
            .expect("native append event timed out");
        let event = receive
            .recv_timeout(remaining)
            .expect("native append event required")
            .unwrap();
        if event.paths.contains(&path) && matches!(event.kind, notify::EventKind::Modify(_)) {
            break;
        }
    }
    source::ingest(&mut store, &path).unwrap();
    assert_eq!(
        store.snapshot().unwrap().direct_tokens.as_deref(),
        Some("53174")
    ); // Two explicit 26587-token records.
    source::ingest(&mut store, &path).unwrap();
    assert_eq!(
        store.snapshot().unwrap().direct_tokens.as_deref(),
        Some("53174")
    );
    assert!(store.snapshot().unwrap().diagnostic.is_none());
}

#[test]
fn conflicting_model_context_invalidates_history_across_restart_and_replay() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("test.sqlite");
    let path = temp.path().join("rollout-model.jsonl");
    fs::write(&path, ACTIVE).unwrap();
    let mut store = Store::open(&db).unwrap();
    source::ingest(&mut store, &path).unwrap();
    let before: (Option<String>, String, String, i64) = store.connection().query_row(
        "SELECT model,normalized,source_path,source_offset FROM observations WHERE response_id='active-response'",
        [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap();
    assert_eq!(before.0.as_deref(), Some("gpt-5.6-terra"));
    append(&path, "{\"type\":\"turn_context\",\"payload\":{\"turn_id\":\"active-turn\",\"model\":\"gpt-6-astra\"}}\n");
    source::ingest(&mut store, &path).unwrap();
    let after: (Option<String>, String, String, i64, Option<String>) = store.connection().query_row(
        "SELECT model,normalized,source_path,source_offset,diagnostic FROM observations WHERE response_id='active-response'",
        [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))).unwrap();
    assert_eq!(after.0, None);
    assert_eq!((after.1, after.2, after.3), (before.1, before.2, before.3));
    assert!(after.4.unwrap().contains("Conflicting model context"));
    drop(store);
    let mut store = Store::open(&db).unwrap();
    let replay = temp.path().join("rollout-model-replay.jsonl");
    fs::write(&replay, ACTIVE).unwrap();
    source::ingest(&mut store, &replay).unwrap();
    let mut next = modern();
    next["payload"]["response_id"] = "later-response".into();
    for value in next["payload"]["thread_token_usage"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        *value = (value.as_i64().unwrap() * 2).into();
    }
    append(&replay, &line(&next));
    source::ingest(&mut store, &replay).unwrap();
    let (observations, attributed): (i64, i64) = store
        .connection()
        .query_row(
            "SELECT COUNT(*),COUNT(model) FROM observations WHERE thread_id='root-active'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((observations, attributed), (2, 0));
    assert_eq!(
        store.snapshot().unwrap().direct_tokens.as_deref(),
        Some("53174")
    );
}

fn historical_record(sequence: i64) -> serde_json::Value {
    let mut value = modern();
    value["timestamp"] = format!("2026-01-01T00:{:02}:{:02}Z", sequence / 60, sequence % 60).into();
    value["payload"]["response_id"] = format!("history-{sequence}").into();
    for counter in value["payload"]["thread_token_usage"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        *counter = (counter.as_i64().unwrap() * sequence).into();
    }
    value
}

fn record_in_store(store: &mut Store, path: &str, value: &serde_json::Value) {
    let (offset, ordinal) = store.checkpoint(path).unwrap();
    let encoded = line(value);
    store
        .line(
            path,
            offset,
            offset + encoded.len() as u64,
            ordinal + 1,
            adapter::decode(encoded.as_bytes()),
        )
        .unwrap();
}

fn settle(store: &mut Store) {
    for _ in 0..1000 {
        if !store.reconcile_pending().unwrap() {
            return;
        }
    }
    panic!("bounded reconciliation did not settle");
}

fn totals(store: &Store) -> (i64, i64, i64) {
    store.connection().query_row("SELECT COALESCE(SUM(total),0),COALESCE(SUM(state='pending'),0),COALESCE(SUM(state='accepted'),0) FROM observations",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap()
}

#[test]
fn metadata_candidates_preserve_usage_provenance_and_ambiguity() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("metadata.sqlite");
    let mut store = Store::open(&db).unwrap();
    let mut metadata = serde_json::json!({"type":"session_meta","payload":{
        "id":"root-active","parent_thread_id":"parent-a","cwd":"C:/workspace/a",
        "source":{"subagent":{"thread_spawn":{"parent_thread_id":"parent-a"}}},
        "prompt":"FORBIDDEN", "instructions":"FORBIDDEN"
    }});
    record_in_store(&mut store, "source", &metadata);
    settle(&mut store);
    let state = |store: &Store| {
        store.connection().query_row("SELECT parent_thread_id,parent_state,location_state FROM sessions WHERE thread_id='root-active'", [], |r| Ok((r.get::<_,Option<String>>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?))).unwrap()
    };
    assert_eq!(
        state(&store),
        (
            Some("parent-a".into()),
            "available".into(),
            "available".into()
        )
    );
    record_in_store(&mut store, "source", &historical_record(1));
    metadata["payload"]["source"]["subagent"]["thread_spawn"]["parent_thread_id"] =
        "parent-b".into();
    metadata["payload"]["cli_version"] = "new-version".into();
    record_in_store(&mut store, "source", &metadata);
    record_in_store(
        &mut store,
        "source",
        &serde_json::json!({"type":"turn_context","payload":{"turn_id":"other-turn","cwd":"C:/workspace/b","workspace_roots":["C:/workspace/b","C:/workspace/c"]}}),
    );
    record_in_store(&mut store, "source", &historical_record(2));
    settle(&mut store);
    assert_eq!(totals(&store), (2 * 26587, 0, 2));
    assert_eq!(
        state(&store),
        (None, "ambiguous".into(), "ambiguous".into())
    );
    assert_eq!(
        store
            .connection()
            .query_row::<i64, _, _>("SELECT halted FROM sources WHERE path='source'", [], |r| r
                .get(0))
            .unwrap(),
        0
    );
    let parents: i64 = store
        .connection()
        .query_row(
            "SELECT COUNT(DISTINCT origin) FROM metadata_evidence WHERE kind='parent'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(parents, 2);
    assert_eq!(
        store
            .connection()
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM metadata_evidence WHERE value LIKE '%FORBIDDEN%'",
                [],
                |r| r.get(0)
            )
            .unwrap(),
        0
    );
    let evidence_before: i64 = store
        .connection()
        .query_row("SELECT COUNT(*) FROM metadata_evidence", [], |r| r.get(0))
        .unwrap();
    drop(store);
    let mut store = Store::open(&db).unwrap();
    assert_eq!(
        state(&store),
        (None, "ambiguous".into(), "ambiguous".into())
    );
    record_in_store(&mut store, "replay", &metadata);
    record_in_store(&mut store, "replay", &historical_record(2));
    assert_eq!(totals(&store), (2 * 26587, 0, 2));
    assert!(
        store
            .connection()
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM metadata_evidence", [], |r| r.get(0))
            .unwrap()
            > evidence_before
    );
    metadata["payload"]["id"] = "different-direct-thread".into();
    record_in_store(&mut store, "source", &metadata);
    record_in_store(&mut store, "source", &historical_record(3));
    assert_eq!(totals(&store), (2 * 26587, 0, 2));
    assert_eq!(
        store
            .connection()
            .query_row::<i64, _, _>("SELECT halted FROM sources WHERE path='source'", [], |r| r
                .get(0))
            .unwrap(),
        1
    );
}

#[test]
fn identity_persistence_keeps_direct_usage_placeholders_and_location_basis() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("identity.sqlite");
    let root = temp.path().join("workspace");
    let worktree = temp.path().join("worktree");
    std::fs::create_dir_all(root.join(".git/worktrees/linked")).unwrap();
    std::fs::create_dir_all(&worktree).unwrap();
    std::fs::write(
        worktree.join(".git"),
        format!("gitdir: {}", root.join(".git/worktrees/linked").display()),
    )
    .unwrap();
    std::fs::write(root.join(".git/worktrees/linked/commondir"), "../..").unwrap();
    let mut store = Store::open(&db).unwrap();
    record_in_store(
        &mut store,
        "child",
        &serde_json::json!({"type":"session_meta","payload":{"id":"root-active","parent_thread_id":"late-parent","cwd":root.join("src"),"workspace_roots":[root]}}),
    );
    record_in_store(&mut store, "child", &historical_record(1));
    assert_eq!(
        store.effective_parent("root-active"),
        Err(crate::hierarchy::ReadError::HierarchyPending)
    );
    assert_eq!(
        store.snapshot().unwrap().direct_tokens.as_deref(),
        Some("26587")
    );
    settle(&mut store);
    assert_eq!(
        store.effective_parent("root-active").unwrap(),
        Some("late-parent".into())
    );
    assert_eq!(
        store
            .connection()
            .query_row::<i64, _, _>(
                "SELECT is_placeholder FROM sessions WHERE thread_id='late-parent'",
                [],
                |r| r.get(0)
            )
            .unwrap(),
        1
    );
    record_in_store(
        &mut store,
        "late",
        &serde_json::json!({"type":"session_meta","payload":{"id":"late-parent"}}),
    );
    record_in_store(
        &mut store,
        "worktree",
        &serde_json::json!({"type":"session_meta","payload":{"id":"other","cwd":worktree}}),
    );
    settle(&mut store);
    assert_eq!(
        store
            .connection()
            .query_row::<i64, _, _>(
                "SELECT is_placeholder FROM sessions WHERE thread_id='late-parent'",
                [],
                |r| r.get(0)
            )
            .unwrap(),
        0
    );
    assert_eq!(
        store
            .connection()
            .query_row::<Option<String>, _, _>(
                "SELECT location_path FROM sessions WHERE thread_id='late-parent'",
                [],
                |r| r.get(0)
            )
            .unwrap(),
        None
    );
    assert_eq!(store.connection().query_row::<i64,_,_>("SELECT COUNT(DISTINCT repository_common_directory) FROM sessions WHERE thread_id IN ('root-active','other')",[],|r|r.get(0)).unwrap(),1);
    assert_eq!(store.connection().query_row::<i64,_,_>("SELECT COUNT(DISTINCT location_path) FROM sessions WHERE thread_id IN ('root-active','other')",[],|r|r.get(0)).unwrap(),2);
    let evidence: i64 = store
        .connection()
        .query_row("SELECT COUNT(*) FROM metadata_evidence", [], |r| r.get(0))
        .unwrap();
    drop(store);
    let mut store = Store::open(&db).unwrap();
    assert_eq!(
        store.effective_parent("root-active").unwrap(),
        Some("late-parent".into())
    );
    assert_eq!(
        store
            .connection()
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM metadata_evidence", [], |r| r.get(0))
            .unwrap(),
        evidence
    );
    record_in_store(
        &mut store,
        "child",
        &serde_json::json!({"type":"turn_context","payload":{"turn_id":"changed","cwd":temp.path().join("elsewhere")}}),
    );
    assert_eq!(
        store
            .connection()
            .query_row::<Option<String>, _, _>(
                "SELECT location_path FROM sessions WHERE thread_id='root-active'",
                [],
                |r| r.get(0)
            )
            .unwrap(),
        None
    );
    assert_eq!(
        store
            .connection()
            .query_row::<Option<String>, _, _>(
                "SELECT repository_common_directory FROM sessions WHERE thread_id='root-active'",
                [],
                |r| r.get(0)
            )
            .unwrap(),
        None
    );
    assert_eq!(totals(&store), (26587, 0, 1));
}

#[test]
fn identity_migration_bootstrap_is_resumable_and_legacy_parent_stays_unresolved() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("legacy-identity.sqlite");
    let connection = rusqlite::Connection::open(&db).unwrap();
    connection
        .execute_batch(include_str!("../migrations/001_initial.sql"))
        .unwrap();
    connection
        .execute_batch(include_str!("../migrations/002_resumable_sources.sql"))
        .unwrap();
    connection
        .execute_batch(include_str!("../migrations/003_snapshot_chronology.sql"))
        .unwrap();
    for n in 0..75 {
        connection
            .execute(
                "INSERT INTO sessions(thread_id,metadata) VALUES(?,?)",
                rusqlite::params![
                    format!("n{n:03}"),
                    serde_json::json!({"parent_thread_id":format!("p{n:03}")}).to_string()
                ],
            )
            .unwrap();
    }
    connection
        .execute_batch(include_str!("../migrations/004_metadata_evidence.sql"))
        .unwrap();
    drop(connection);
    let mut store = Store::open(&db).unwrap();
    assert_eq!(
        store.effective_parent("n000"),
        Err(crate::hierarchy::ReadError::HierarchyPending)
    );
    assert!(store.reconcile_pending().unwrap());
    drop(store);
    let mut store = Store::open(&db).unwrap();
    settle(&mut store);
    assert_eq!(store.effective_parent("n000").unwrap(), None);
    assert_eq!(
        store
            .connection()
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM sessions WHERE parent_state='legacy'",
                [],
                |r| r.get(0)
            )
            .unwrap(),
        75
    );
    assert_eq!(
        store
            .connection()
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM sessions WHERE is_placeholder=1",
                [],
                |r| r.get(0)
            )
            .unwrap(),
        75
    );
    record_in_store(
        &mut store,
        "current",
        &serde_json::json!({"type":"session_meta","payload":{"id":"n000","parent_thread_id":"p000"}}),
    );
    settle(&mut store);
    assert_eq!(store.effective_parent("n000").unwrap(), Some("p000".into()));
    record_in_store(
        &mut store,
        "conflict",
        &serde_json::json!({"type":"session_meta","payload":{"id":"n001","parent_thread_id":"different"}}),
    );
    settle(&mut store);
    assert_eq!(store.effective_parent("n001").unwrap(), None);
    assert_eq!(
        store
            .connection()
            .query_row::<String, _, _>(
                "SELECT parent_state FROM sessions WHERE thread_id='n001'",
                [],
                |r| r.get(0)
            )
            .unwrap(),
        "ambiguous"
    );
}

#[test]
fn metadata_migration_backfills_only_recoverable_projections() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("v3.sqlite");
    let connection = rusqlite::Connection::open(&db).unwrap();
    connection
        .execute_batch(include_str!("../migrations/001_initial.sql"))
        .unwrap();
    connection
        .execute_batch(include_str!("../migrations/002_resumable_sources.sql"))
        .unwrap();
    connection
        .execute_batch(include_str!("../migrations/003_snapshot_chronology.sql"))
        .unwrap();
    connection
        .execute(
            "INSERT INTO sessions(thread_id,metadata) VALUES('root-active',?)",
            [r#"{"id":"root-active","parent_thread_id":"parent","cwd":"C:/project"}"#],
        )
        .unwrap();
    connection.execute("INSERT INTO sources(path,thread_id,halted,diagnostic) VALUES('old','root-active',1,'Conflicting session identity or metadata')", []).unwrap();
    drop(connection);
    let mut store = Store::open(&db).unwrap();
    assert_eq!(
        store
            .connection()
            .query_row::<i64, _, _>("SELECT halted FROM sources WHERE path='old'", [], |r| r
                .get(0))
            .unwrap(),
        1
    );
    assert_eq!(
        store
            .connection()
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM metadata_evidence WHERE origin='legacy_projection'",
                [],
                |r| r.get(0)
            )
            .unwrap(),
        2
    );
    assert_eq!(
        store
            .connection()
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM metadata_evidence WHERE kind='workspace_root'",
                [],
                |r| r.get(0)
            )
            .unwrap(),
        0
    );
    record_in_store(&mut store, "usage", &historical_record(1));
    assert_eq!(
        store.snapshot().unwrap().direct_tokens.as_deref(),
        Some("26587")
    );
}

#[test]
fn metadata_historical_suppression_requires_clean_fresh_generation_replay() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("replay.sqlite");
    let path = temp.path().join("rollout-replay.jsonl");
    let key = path.to_string_lossy();
    let meta =
        serde_json::json!({"type":"session_meta","payload":{"id":"root-active","cwd":"C:/old"}});
    let first = line(&meta) + &line(&historical_record(1));
    fs::write(&path, &first).unwrap();
    let mut store = Store::open(&db).unwrap();
    source::ingest(&mut store, &path).unwrap();
    // Reproduce v3's conflated halt, with immutable confirmed usage preceding it.
    store.connection().execute("UPDATE sources SET halted=1,diagnostic='Conflicting session identity or metadata' WHERE path=?", [&*key]).unwrap();
    append(&path, &line(&historical_record(2)));
    source::ingest(&mut store, &path).unwrap();
    assert_eq!(totals(&store), (26587, 0, 1));
    let original: (String,i64,i64) = store.connection().query_row("SELECT normalized,source_generation,source_offset FROM observations WHERE response_id='history-2'", [], |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
    drop(store);
    let mut store = Store::open(&db).unwrap();
    source::ingest(&mut store, &path).unwrap();
    assert_eq!(totals(&store), (26587, 0, 1));
    assert!(store.snapshot().unwrap().coverage.contains("Incomplete"));

    // Replacement triggers the existing offset-zero generation recovery. A true
    // identity conflict in its prefix must still prevent duplicate-row recovery.
    let wrong = serde_json::json!({"type":"session_meta","payload":{"id":"other-thread"}});
    fs::write(
        &path,
        first.clone() + &line(&wrong) + &line(&historical_record(2)),
    )
    .unwrap();
    source::ingest(&mut store, &path).unwrap();
    assert_eq!(totals(&store), (26587, 0, 1));
    assert_eq!(
        store
            .connection()
            .query_row::<i64, _, _>("SELECT halted FROM sources WHERE path=?", [&*key], |r| r
                .get(0))
            .unwrap(),
        1
    );

    let changed = serde_json::json!({"type":"session_meta","payload":{"id":"root-active","parent_thread_id":"parent-a","source":{"subagent":{"thread_spawn":{"parent_thread_id":"parent-b"}}},"cwd":"C:/new"}});
    fs::write(
        &path,
        line(&changed) + &line(&historical_record(1)) + &line(&historical_record(2)),
    )
    .unwrap();
    source::ingest(&mut store, &path).unwrap();
    assert_eq!(totals(&store), (2 * 26587, 0, 2));
    let after: (String,i64,i64) = store.connection().query_row("SELECT normalized,source_generation,source_offset FROM observations WHERE response_id='history-2'", [], |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
    assert_eq!(original, after);
    source::ingest(&mut store, &path).unwrap();
    assert_eq!(totals(&store), (2 * 26587, 0, 2));
    // Explicit rejects are sticky even when a clean replay presents identical data.
    store.connection().execute("UPDATE observations SET accepted=0,total=NULL,state='rejected',diagnostic='Conflicting usage identity; pending usage unavailable' WHERE response_id='history-2'", []).unwrap();
    fs::write(&path, first + &line(&historical_record(2))).unwrap();
    source::ingest(&mut store, &path).unwrap();
    assert_eq!(totals(&store), (26587, 0, 1));
}

#[test]
fn metadata_replay_invalid_duplicate_prefix_keeps_later_usage_suppressed() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("invalid-prefix.sqlite");
    let path = temp.path().join("rollout-invalid-prefix.jsonl");
    let key = path.to_string_lossy();
    let meta = serde_json::json!({"type":"session_meta","payload":{"id":"root-active"}});
    let mut invalid = historical_record(1);
    invalid["payload"]["usage"]
        .as_object_mut()
        .unwrap()
        .remove("cached_input_tokens");
    fs::write(
        &path,
        line(&meta) + &line(&invalid) + &line(&historical_record(2)),
    )
    .unwrap();
    let mut store = Store::open(&db).unwrap();
    source::ingest(&mut store, &path).unwrap();
    assert_eq!(totals(&store), (0, 0, 0));
    let rejected = |store: &Store| {
        store
            .connection()
            .prepare("SELECT response_id,state,diagnostic,normalized FROM observations ORDER BY id")
            .unwrap()
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    let before = rejected(&store);
    assert_eq!(before.len(), 2);
    assert_eq!(
        before[1].2,
        "Source accounting stopped after an unsupported record"
    );
    let state = store.source_state(&key).unwrap();
    store
        .restart_source(
            &key,
            state.generation,
            state.identity.as_deref(),
            fs::metadata(&path).unwrap().len(),
        )
        .unwrap();
    drop(store);
    let mut store = Store::open(&db).unwrap();
    source::ingest(&mut store, &path).unwrap();
    assert_eq!(totals(&store), (0, 0, 0));
    assert_eq!(rejected(&store), before);
    assert_eq!(
        store
            .connection()
            .query_row::<i64, _, _>("SELECT halted FROM sources WHERE path=?", [&*key], |r| r
                .get(0))
            .unwrap(),
        1
    );
}

#[test]
fn historical_arrival_permutations_bridge_both_directions_without_changing_anchors() {
    for order in [
        [1, 2, 3],
        [1, 3, 2],
        [2, 1, 3],
        [2, 3, 1],
        [3, 1, 2],
        [3, 2, 1],
    ] {
        let temp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&temp.path().join("history.sqlite")).unwrap();
        record_in_store(&mut store, "first", &historical_record(order[0]));
        let anchor: (String, String, i64) = store
            .connection()
            .query_row(
                "SELECT normalized,source_path,total FROM observations WHERE accepted=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        for sequence in &order[1..] {
            record_in_store(
                &mut store,
                &format!("source-{sequence}"),
                &historical_record(*sequence),
            );
        }
        settle(&mut store);
        assert_eq!(totals(&store), (3 * 26587, 0, 3), "arrival order {order:?}");
        let after: (String, String, i64) = store
            .connection()
            .query_row(
                "SELECT normalized,source_path,total FROM observations WHERE id=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(anchor, after);
    }
}

#[test]
fn unresolved_gap_stays_pending_and_duplicate_remains_promotable() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("gap.sqlite")).unwrap();
    record_in_store(&mut store, "live", &historical_record(3));
    record_in_store(&mut store, "old", &historical_record(1));
    record_in_store(&mut store, "replay", &historical_record(1));
    settle(&mut store);
    assert_eq!(totals(&store), (26587, 1, 1));
    let checkpoint = store.checkpoint("replay").unwrap();
    assert!(checkpoint.0 > 0);
    assert!(store
        .snapshot()
        .unwrap()
        .diagnostic
        .unwrap()
        .contains("pending"));
    record_in_store(&mut store, "bridge", &historical_record(2));
    settle(&mut store);
    assert_eq!(totals(&store), (3 * 26587, 0, 3));
}

#[test]
fn canonical_timestamps_and_equal_time_counter_evidence_are_order_independent() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("time.sqlite")).unwrap();
    let mut first = historical_record(1);
    first["timestamp"] = "2026-01-01T01:00:00+01:00".into();
    record_in_store(&mut store, "one", &first);
    first["timestamp"] = "2026-01-01T00:00:00.000000000Z".into();
    record_in_store(&mut store, "duplicate", &first);
    let mut second = historical_record(2);
    second["timestamp"] = "2026-01-01T00:00:00Z".into();
    record_in_store(&mut store, "other", &second);
    assert_eq!(totals(&store), (2 * 26587, 0, 2));
    let mut ambiguous = historical_record(3);
    ambiguous["timestamp"] = "2026-01-01T00:00:00Z".into();
    // Both endpoints are individually valid, but their category vectors cross.
    ambiguous["payload"]["thread_token_usage"]["reasoning_output_tokens"] = 0.into();
    ambiguous["payload"]["usage"]["reasoning_output_tokens"] = 0.into();
    record_in_store(&mut store, "ambiguous", &ambiguous);
    settle(&mut store);
    assert_eq!(totals(&store), (2 * 26587, 1, 2));
    assert!(adapter::observation_time("2026-99-01T00:00:00Z").is_err());
}

#[test]
fn pending_identity_conflict_cannot_be_promoted_after_bridge_arrives() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("conflict.sqlite")).unwrap();
    record_in_store(&mut store, "live", &historical_record(3));
    record_in_store(&mut store, "old", &historical_record(1));
    let mut conflict = historical_record(1);
    conflict["payload"]["usage"]["reasoning_output_tokens"] = 0.into();
    record_in_store(&mut store, "conflict", &conflict);
    record_in_store(&mut store, "bridge", &historical_record(2));
    settle(&mut store);
    assert_eq!(totals(&store), (2 * 26587, 0, 2));
    let rejected: i64 = store
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM observations WHERE state='rejected'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(rejected, 1);
}

#[test]
fn recovery_metadata_and_batch_rollback_preserve_only_confirmed_usage() {
    use crate::storage::{InputLine, SourceProgress};
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("recovery.sqlite");
    let mut store = Store::open(&db).unwrap();
    store.source_state("source").unwrap();
    let first = line(&historical_record(1));
    let progress = SourceProgress {
        offset: first.len() as u64,
        ordinal: 1,
        known_size: first.len() as u64 + 31,
        tail_length: 31,
        verification_length: 32,
        verification_hash: Some([0xAB; 32]),
        ..Default::default()
    };
    store
        .batch(
            "source",
            0,
            vec![InputLine {
                start: 0,
                end: first.len() as u64,
                ordinal: 1,
                record: adapter::decode(first.as_bytes()),
            }],
            progress.clone(),
        )
        .unwrap();
    drop(store);
    let mut store = Store::open(&db).unwrap();
    assert_eq!(store.source_state("source").unwrap().progress, progress);
    store.connection().execute_batch("CREATE TRIGGER fail_checkpoint BEFORE UPDATE OF offset ON sources BEGIN SELECT RAISE(ABORT, 'interrupted batch'); END;").unwrap();
    let second = line(&historical_record(2));
    let end = progress.offset + second.len() as u64;
    assert!(store
        .batch(
            "source",
            0,
            vec![InputLine {
                start: progress.offset,
                end,
                ordinal: 2,
                record: adapter::decode(second.as_bytes())
            }],
            SourceProgress {
                offset: end,
                ordinal: 2,
                known_size: end,
                ..Default::default()
            }
        )
        .is_err());
    assert_eq!(totals(&store), (26587, 0, 1));
    assert_eq!(store.source_state("source").unwrap().progress, progress);
    store
        .connection()
        .execute_batch("DROP TRIGGER fail_checkpoint")
        .unwrap();
    store
        .restart_source("source", 0, Some("volume:123-file:456"), first.len() as u64)
        .unwrap();
    assert!(store
        .batch("source", 0, vec![], SourceProgress::default())
        .is_err());
    record_in_store(&mut store, "source", &historical_record(1));
    assert_eq!(totals(&store), (26587, 0, 1));
    record_in_store(&mut store, "source", &historical_record(2));
    assert_eq!(totals(&store), (2 * 26587, 0, 2));
    store.source_removed("source", 1).unwrap();
    assert_eq!(
        store.source_state("source").unwrap().progress.tail_length,
        0
    );
    assert_eq!(totals(&store), (2 * 26587, 0, 2));
    let generations: Vec<i64> = store
        .connection()
        .prepare("SELECT source_generation FROM observations ORDER BY id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(generations, vec![0, 1]);
    // The recovery schema can retain boundaries and a fixed digest, never raw tail text.
    let columns: Vec<String> = store
        .connection()
        .prepare("PRAGMA table_info(sources)")
        .unwrap()
        .query_map([], |r| r.get(1))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(!columns
        .iter()
        .any(|name| name == "tail_bytes" || name == "tail_content"));
}

#[test]
fn bounded_pending_promotion_resumes_after_restart() {
    use crate::storage::{InputLine, SourceProgress};
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("resume.sqlite");
    let mut store = Store::open(&db).unwrap();
    record_in_store(&mut store, "anchor", &historical_record(64));
    store.source_state("history").unwrap();
    let mut offset = 0;
    let inputs = (1..64)
        .map(|sequence| {
            let encoded = line(&historical_record(sequence));
            let start = offset;
            offset += encoded.len() as u64;
            InputLine {
                start,
                end: offset,
                ordinal: sequence,
                record: adapter::decode(encoded.as_bytes()),
            }
        })
        .collect();
    store
        .batch(
            "history",
            0,
            inputs,
            SourceProgress {
                offset,
                ordinal: 63,
                known_size: offset,
                ..Default::default()
            },
        )
        .unwrap();
    let unfinished: i64 = store
        .connection()
        .query_row("SELECT COUNT(*) FROM reconciliation_work", [], |r| r.get(0))
        .unwrap();
    assert_eq!(unfinished, 1);
    assert!(totals(&store).1 > 0);
    drop(store);
    let mut store = Store::open(&db).unwrap();
    settle(&mut store);
    assert_eq!(totals(&store), (64 * 26587, 0, 64));
    assert_eq!(store.checkpoint("history").unwrap(), (offset, 63));
}

#[test]
fn version_one_migration_preserves_usage_and_promotes_its_pending_gap() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("migration.sqlite");
    let connection = rusqlite::Connection::open(&db).unwrap();
    connection
        .execute_batch(include_str!("../migrations/001_initial.sql"))
        .unwrap();
    connection
        .execute(
            "INSERT INTO sources(path,offset,ordinal,thread_id,halted,diagnostic) VALUES('old',123,1,'root-active',1,'Unexplained thread gap; remaining source usage unavailable')",
            [],
        )
        .unwrap();
    connection
        .execute("INSERT INTO sessions(thread_id) VALUES('root-active')", [])
        .unwrap();
    connection.execute("INSERT INTO sources(path,thread_id,halted,diagnostic) VALUES('malformed','root-active',1,'Malformed JSONL record')", []).unwrap();
    for sequence in [1, 3, 4, 5, 6, 7] {
        let mut record = historical_record(sequence);
        if sequence == 5 {
            record["payload"]["usage"]["total_tokens"] = (-1).into();
        }
        let adapter::Record::Usage { timestamp, usage } =
            adapter::decode(line(&record).as_bytes()).unwrap()
        else {
            panic!()
        };
        let reason = match sequence {
            1 => None,
            3 => Some("Unexplained thread gap; remaining source usage unavailable"),
            6 => Some("Conflicting usage identity; duplicate endpoint was not counted"),
            _ => Some("Source accounting stopped after an unsupported record"),
        };
        connection.execute("INSERT INTO observations(thread_id,endpoint,response_id,timestamp,normalized,adapter,source_path,source_offset,source_ordinal,accepted,total,diagnostic) VALUES(?,?,?,?,?,'modern-1',?,?,?,?,?,?)",rusqlite::params![usage.thread_id,serde_json::to_string(&usage.thread_token_usage).unwrap(),usage.response_id,timestamp,serde_json::to_string(&usage).unwrap(),if sequence==7 {"malformed"} else {"old"},sequence,sequence,sequence==1,if sequence==1 {Some(26587)} else {None},reason]).unwrap();
    }
    drop(connection);
    let mut store = Store::open(&db).unwrap();
    assert_eq!(store.checkpoint("old").unwrap(), (123, 1));
    assert_eq!(totals(&store), (26587, 2, 1));
    record_in_store(&mut store, "replay", &historical_record(4));
    assert_eq!(totals(&store), (26587, 2, 1));
    record_in_store(&mut store, "bridge", &historical_record(2));
    settle(&mut store);
    assert_eq!(totals(&store), (4 * 26587, 0, 4));
    let rejected: i64 = store
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM observations WHERE state='rejected'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(rejected, 3); // Invalid usage, explicit conflict, and a malformed-source halt stay rejected.
    record_in_store(&mut store, "replay", &historical_record(4));
    assert_eq!(totals(&store), (4 * 26587, 0, 4));
    drop(store);
    let store = Store::open(&db).unwrap();
    let version: i64 = store
        .connection()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 9);
    assert_eq!(totals(&store), (4 * 26587, 0, 4));
}

#[test]
fn equal_time_large_groups_use_offsets_only_within_the_same_generation() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("equal.sqlite")).unwrap();
    for sequence in 1..=100 {
        let mut value = historical_record(sequence);
        value["timestamp"] = "2026-01-01T00:00:00Z".into();
        record_in_store(&mut store, "same-source", &value);
    }
    assert_eq!(totals(&store), (100 * 26587, 0, 100));

    let mut other = Store::open(&temp.path().join("generation.sqlite")).unwrap();
    let mut first = historical_record(1);
    let mut second = historical_record(2);
    first["timestamp"] = "2026-01-01T00:00:00Z".into();
    second["timestamp"] = "2026-01-01T00:00:00Z".into();
    record_in_store(&mut other, "same-source", &second);
    // In this generation the lower endpoint is later by offset: cannot count it.
    record_in_store(&mut other, "same-source", &first);
    assert_eq!(totals(&other), (26587, 1, 1));
    // A newly available earlier source is independently ordered by its counters.
    let mut restored = Store::open(&temp.path().join("restored.sqlite")).unwrap();
    record_in_store(&mut restored, "same-source", &second);
    restored
        .restart_source("same-source", 0, Some("replacement-id"), 0)
        .unwrap();
    record_in_store(&mut restored, "same-source", &first);
    assert_eq!(totals(&restored), (2 * 26587, 0, 2));
}

#[test]
fn ignored_content_and_malformed_records_never_enter_recovery_storage() {
    use crate::storage::{InputLine, SourceProgress};
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("privacy.sqlite");
    let mut store = Store::open(&db).unwrap();
    store.source_state("source").unwrap();
    let content =
        b"{\"type\":\"response_item\",\"payload\":{\"content\":\"PRIVATE_SENTINEL_934759\"}}\n";
    let malformed = b"{PRIVATE_SENTINEL_934759}\n";
    let end = (content.len() + malformed.len()) as u64;
    store
        .batch(
            "source",
            0,
            vec![
                InputLine {
                    start: 0,
                    end: content.len() as u64,
                    ordinal: 1,
                    record: adapter::decode(content),
                },
                InputLine {
                    start: content.len() as u64,
                    end,
                    ordinal: 2,
                    record: adapter::decode(malformed),
                },
            ],
            SourceProgress {
                offset: end,
                ordinal: 2,
                known_size: end + 4,
                tail_length: 4,
                verification_length: 32,
                verification_hash: Some([0xCD; 32]),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(store
        .snapshot()
        .unwrap()
        .diagnostic
        .unwrap()
        .contains("Malformed"));
    store
        .connection()
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
    drop(store);
    let persisted = fs::read(db).unwrap();
    assert!(!persisted
        .windows(b"PRIVATE_SENTINEL_934759".len())
        .any(|window| window == b"PRIVATE_SENTINEL_934759"));
}
