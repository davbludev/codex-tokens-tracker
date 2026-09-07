use crate::{adapter, source, storage::Store};
use std::{fs, io::Write};

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
        store.snapshot().unwrap().direct_tokens.as_deref(),
        Some("30444")
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
    assert_eq!(metadata["parent_thread_id"], "parent");
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
    store.connection().query_row("SELECT COALESCE(SUM(total),0),SUM(state='pending'),SUM(state='accepted') FROM observations",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap()
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
    assert_eq!(version, 2);
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
