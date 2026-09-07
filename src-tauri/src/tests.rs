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
