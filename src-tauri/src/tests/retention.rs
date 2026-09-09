use super::{
    pricing::{context, drain, prices, time, tokens, usage},
    record_in_store,
    weekly::limit,
};
use crate::{
    commands::runtime::Work,
    storage::{
        retention::{Retired, RETENTION_BATCH},
        Store,
    },
};
use serde_json::json;
use std::time::Instant;

fn now() -> (i64, u32) {
    time("2026-09-08T12:00:00Z")
}
fn count(store: &Store, sql: &str) -> i64 {
    store
        .connection()
        .query_row(sql, [], |row| row.get(0))
        .unwrap()
}
fn weekly(store: &mut Store, timestamp: &str) {
    limit(store, timestamp, "10", "codex", 10080, "secondary", None);
}
/// Replay a usage record without expecting it to be stored.
fn replay(store: &mut Store, path: &str, thread: &str, sequence: i64, timestamp: &str) {
    let usage = tokens([100, 20, 0, 40, 10, 140]);
    let endpoint = tokens([100, 20, 0, 40, 10, 140].map(|n| n * sequence));
    record_in_store(
        store,
        path,
        &json!({"type":"token_usage_record","timestamp":timestamp,"payload":{"thread_id":thread,"turn_id":"turn","response_id":format!("response-{sequence}"),"usage":usage,"thread_token_usage":endpoint}}),
    );
}

#[test]
fn retention_retires_only_usage_older_than_45_days_in_bounded_batches() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("retention.sqlite")).unwrap();
    context(&mut store, "a", "thread", Some("model"));
    store
        .save_model_price_at("model", prices(), true, time("2025-01-01T00:00:00Z"))
        .unwrap();
    for sequence in 1..=300 {
        usage(
            &mut store,
            "a",
            "thread",
            sequence,
            &format!("2025-10-01T00:{:02}:{:02}Z", sequence / 60, sequence % 60),
        );
    }
    usage(&mut store, "a", "thread", 301, "2026-01-01T00:00:01Z");
    let newest = usage(&mut store, "a", "thread", 302, "2026-01-01T00:00:02Z");
    // The newest rowid is old but survives, so rowids can never restart.
    context(&mut store, "late", "late-thread", Some("model"));
    let late = usage(&mut store, "late", "late-thread", 1, "2025-10-02T00:00:00Z");
    weekly(&mut store, "2025-10-01T00:00:00Z");
    // Cutoff is 2025-11-17T00:00:02Z: offsets are compared exactly, not lexically.
    weekly(&mut store, "2025-11-17T01:00:01+01:00");
    weekly(&mut store, "2025-11-17T01:00:03+01:00");
    weekly(&mut store, "2026-01-01T00:00:00Z");
    drain(&mut store);
    assert_eq!(
        count(&store, "SELECT COUNT(*) FROM observation_valuations"),
        303
    );
    assert_eq!(count(&store, "SELECT COUNT(*) FROM limit_samples"), 4);

    assert_eq!(
        store.retire_expired(now(), RETENTION_BATCH).unwrap(),
        Retired {
            observations: 256,
            samples: 2
        }
    );
    assert_eq!(
        store.retire_expired(now(), RETENTION_BATCH).unwrap(),
        Retired {
            observations: 44,
            samples: 0
        }
    );
    assert_eq!(
        store.retire_expired(now(), RETENTION_BATCH).unwrap(),
        Retired::default()
    );
    assert_eq!(count(&store, "SELECT COUNT(*) FROM observations"), 3);
    assert_eq!(
        count(&store, "SELECT COUNT(*) FROM observation_valuations"),
        3
    );
    assert_eq!(count(&store, "SELECT COUNT(*) FROM reconciliation_work"), 0);
    assert_eq!(count(&store, "SELECT MAX(id) FROM observations"), late);
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM limit_samples WHERE timestamp>='2025-11-17T01:00:03+01:00'"
        ),
        2
    );
    assert_eq!(count(&store, "SELECT COUNT(*) FROM limit_samples"), 2);
    assert_eq!(
        count(&store, "SELECT floor_seconds FROM retention_control"),
        time("2025-11-17T00:00:02Z").0
    );
    assert_eq!(super::totals(&store), (140 * 3, 0, 3));
    // A clock far ahead of the stored history retires nothing more.
    assert_eq!(
        store
            .retire_expired(time("2030-01-01T00:00:00Z"), RETENTION_BATCH)
            .unwrap(),
        Retired::default()
    );
    assert_eq!(count(&store, "SELECT COUNT(*) FROM observations"), 3);
    // A valuation still leaves only together with its observation.
    assert!(store
        .connection()
        .execute(
            "DELETE FROM observation_valuations WHERE observation_id=?",
            [newest]
        )
        .is_err());
    let plan = store
        .connection()
        .prepare("EXPLAIN QUERY PLAN SELECT id FROM observations WHERE time_seconds IS NOT NULL AND time_nanos IS NOT NULL AND (time_seconds,time_nanos)<(?1,?2) AND id<(SELECT MAX(id) FROM observations) ORDER BY time_seconds,time_nanos LIMIT ?3")
        .unwrap()
        .query_map([0, 0, 1], |row| row.get::<_, String>(3))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert!(
        plan.iter().any(|step| step.contains("observation_latest")),
        "{plan:?}"
    );
}

#[test]
fn retention_floor_blocks_replay_and_clears_source_gap_diagnostic() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("floor.sqlite")).unwrap();
    context(&mut store, "a", "thread", Some("model"));
    usage(&mut store, "a", "thread", 1, "2025-10-01T00:00:01Z");
    // Sequence 2 never arrives, so sequence 3 stays pending with a gap.
    usage(&mut store, "a", "thread", 3, "2025-10-01T00:00:03Z");
    assert_eq!(super::totals(&store), (140, 1, 1));
    assert_eq!(
        store
            .connection()
            .query_row::<Option<String>, _, _>(
                "SELECT diagnostic FROM sources WHERE path='a'",
                [],
                |row| row.get(0)
            )
            .unwrap()
            .as_deref(),
        Some(crate::storage::GAP)
    );
    context(&mut store, "b", "other", Some("model"));
    usage(&mut store, "b", "other", 1, "2026-01-01T00:00:00Z");

    assert_eq!(
        store.retire_expired(now(), RETENTION_BATCH).unwrap(),
        Retired {
            observations: 2,
            samples: 0
        }
    );
    assert_eq!(super::totals(&store), (140, 0, 1));
    assert!(store
        .connection()
        .query_row::<Option<String>, _, _>(
            "SELECT diagnostic FROM sources WHERE path='a'",
            [],
            |row| row.get(0)
        )
        .unwrap()
        .is_none());

    // An archived copy of the retired history replays nothing and never
    // marks the session incomplete; the source still learns its thread.
    context(&mut store, "replay", "thread", Some("model"));
    replay(&mut store, "replay", "thread", 1, "2025-10-01T00:00:01Z");
    replay(&mut store, "replay", "thread", 2, "2025-10-01T00:00:02Z");
    weekly(&mut store, "2025-10-01T00:00:00Z");
    weekly(&mut store, "2026-01-01T00:00:00Z");
    assert_eq!(count(&store, "SELECT COUNT(*) FROM observations"), 1);
    assert_eq!(count(&store, "SELECT COUNT(*) FROM limit_samples"), 1);
    assert_eq!(
        count(
            &store,
            "SELECT incomplete FROM sessions WHERE thread_id='thread'"
        ),
        0
    );
    assert_eq!(
        store
            .connection()
            .query_row::<Option<String>, _, _>(
                "SELECT thread_id FROM sources WHERE path='replay'",
                [],
                |row| row.get(0)
            )
            .unwrap()
            .as_deref(),
        Some("thread")
    );
    // Usage above the floor still imports normally.
    replay(&mut store, "replay", "thread", 4, "2026-01-01T00:00:04Z");
    assert_eq!(count(&store, "SELECT COUNT(*) FROM observations"), 2);
}

#[test]
fn retention_lane_runs_when_due_and_honours_pause() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("lane.sqlite")).unwrap();
    context(&mut store, "a", "thread", Some("model"));
    usage(&mut store, "a", "thread", 1, "2025-10-01T00:00:01Z");
    usage(&mut store, "a", "thread", 2, "2026-01-01T00:00:00Z");
    let started = Instant::now();
    let mut work = Work::without_sources();
    for _ in 0..60 {
        work.step(&mut store, Instant::now()).unwrap();
    }
    assert_eq!(count(&store, "SELECT COUNT(*) FROM observations"), 1);
    assert!(!work.busy());
    assert_eq!(work.deadline(), None);
    assert!(work
        .retention_deadline()
        .is_some_and(|due| due > started + crate::commands::runtime::RETENTION_INTERVAL / 2));
    work.set_paused(true);
    assert_eq!(work.retention_deadline(), None);
}

#[test]
fn migration_010_enqueues_one_valuation_job_per_priced_model() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("migrate.sqlite");
    let mut store = Store::open(&path).unwrap();
    context(&mut store, "a", "thread", Some("m1"));
    context(&mut store, "b", "other", Some("m2"));
    let far = usage(&mut store, "a", "thread", 1, "2025-12-20T00:00:00Z");
    let near = usage(&mut store, "a", "thread", 2, "2026-01-01T00:00:00Z");
    let other = usage(&mut store, "b", "other", 1, "2026-01-02T00:00:00Z");
    let first = store
        .save_model_price_at("m1", prices(), false, time("2026-01-03T00:00:00Z"))
        .unwrap();
    let mut changed = prices();
    changed.input = "2".into();
    let second = store
        .save_model_price_at("m1", changed, false, time("2026-01-04T00:00:00Z"))
        .unwrap();
    let only = store
        .save_model_price_at("m2", prices(), false, time("2026-01-05T00:00:00Z"))
        .unwrap();
    // Shape a database that was upgraded before the reach-back rule existed.
    store
        .connection()
        .execute_batch("DELETE FROM pricing_work; DROP TABLE retention_control; DROP INDEX limit_sample_timestamp; ALTER TABLE turn_contexts DROP COLUMN effort; ALTER TABLE observations DROP COLUMN effort; PRAGMA user_version=9;")
        .unwrap();
    drop(store);
    let mut store = Store::open(&path).unwrap();
    assert_eq!(count(&store, "PRAGMA user_version"), 11);
    let jobs = store
        .connection()
        .prepare("SELECT version_id,after_id,through_id FROM pricing_work ORDER BY version_id")
        .unwrap()
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(jobs, vec![(second.id, 0, other), (only.id, 0, other)]);
    drain(&mut store);
    assert!(store.observation_valuation(far).unwrap().is_none());
    let value = store.observation_valuation(near).unwrap().unwrap();
    assert_eq!(
        (value.version_id, value.amount.as_str()),
        (first.id, "210000000")
    );
    let value = store.observation_valuation(other).unwrap().unwrap();
    assert_eq!(
        (value.version_id, value.amount.as_str()),
        (only.id, "210000000")
    );
}
