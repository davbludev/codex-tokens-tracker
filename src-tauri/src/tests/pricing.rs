use super::record_in_store;
use crate::{
    adapter::{self, Counter, Tokens},
    pricing::{CacheWritePolicy, Error, PriceInput, ReasoningPolicy},
    storage::{pricing::PRICING_BATCH_SIZE, Store},
};
use serde_json::json;

fn prices() -> PriceInput {
    PriceInput {
        input: "1".into(),
        cached_input: "0.5".into(),
        cache_write: "2".into(),
        output: "3".into(),
        reasoning: None,
        reasoning_policy: ReasoningPolicy::Included,
        cache_write_policy: CacheWritePolicy::Unknown,
    }
}
fn tokens(values: [i64; 6]) -> Tokens {
    let [input, cached, writes, output, reasoning, total] = values.map(Counter::Known);
    Tokens {
        input_tokens: input,
        cached_input_tokens: cached,
        cache_write_input_tokens: writes,
        output_tokens: output,
        reasoning_output_tokens: reasoning,
        total_tokens: total,
    }
}
fn time(value: &str) -> (i64, u32) {
    adapter::observation_time(value).unwrap()
}
fn context(store: &mut Store, path: &str, thread: &str, model: Option<&str>) {
    record_in_store(
        store,
        path,
        &json!({"type":"session_meta","payload":{"id":thread}}),
    );
    record_in_store(
        store,
        path,
        &json!({"type":"turn_context","payload":{"turn_id":"turn","model":model}}),
    );
}
fn usage(store: &mut Store, path: &str, thread: &str, sequence: i64, timestamp: &str) -> i64 {
    let usage = tokens([100, 20, 0, 40, 10, 140]);
    let endpoint = tokens([100, 20, 0, 40, 10, 140].map(|n| n * sequence));
    record_in_store(
        store,
        path,
        &json!({"type":"token_usage_record","timestamp":timestamp,"payload":{"thread_id":thread,"turn_id":"turn","response_id":format!("response-{sequence}"),"usage":usage,"thread_token_usage":endpoint}}),
    );
    store
        .connection()
        .query_row(
            "SELECT id FROM observations WHERE thread_id=? AND response_id=?",
            rusqlite::params![thread, format!("response-{sequence}")],
            |r| r.get(0),
        )
        .unwrap()
}
fn drain(store: &mut Store) {
    for _ in 0..1000 {
        if !store.pricing_work_pending().unwrap() {
            return;
        }
        assert!(store.process_pricing_work().unwrap() <= PRICING_BATCH_SIZE);
    }
    panic!("pricing work did not finish");
}

#[test]
fn pricing_decimal_validation_and_exact_subtraction() {
    let original = tokens([100, 20, 0, 40, 10, 140]);
    assert_eq!(
        prices().validate().unwrap().value(&original),
        Ok(210_000_000)
    ); // 80*1 + 20*.5 + 40*3 micro-USD/million
    let mut input = prices();
    input.reasoning_policy = ReasoningPolicy::Separate;
    assert_eq!(input.validate().unwrap_err(), Error::ReasoningRate);
    input.reasoning = Some("5".into());
    assert_eq!(input.validate().unwrap().value(&original), Ok(230_000_000)); // output:30*3 + reasoning:10*5
    for invalid in [
        "",
        "-1",
        "+1",
        ".5",
        "1.",
        "1e3",
        "NaN",
        " 1",
        "1.0000001",
        "9223372036854.775808",
    ] {
        input.input = invalid.into();
        assert!(
            matches!(input.validate(), Err(Error::InvalidRate { field: "input" })),
            "{invalid}"
        );
    }
    input.input = "9223372036854.775807".into();
    assert!(input.validate().is_ok());
    input = prices();
    input.input = "0.000001".into();
    input.cached_input = "0".into();
    input.output = "0".into();
    assert_eq!(input.validate().unwrap().value(&original), Ok(80));
    let mut wire = serde_json::to_value(prices()).unwrap();
    wire["effectiveAt"] = json!("2020-01-01T00:00:00Z");
    assert!(serde_json::from_value::<PriceInput>(wire).is_err());
}

#[test]
fn pricing_category_policies_unknown_missing_zero_and_overflow() {
    let mut input = prices();
    let writes = tokens([100, 20, 10, 40, 10, 140]);
    assert_eq!(
        input.validate().unwrap().value(&writes),
        Err(Error::UnknownCacheWrite)
    );
    input.cache_write_policy = CacheWritePolicy::IncludedInputDisjoint;
    assert_eq!(input.validate().unwrap().value(&writes), Ok(220_000_000));
    input.cache_write_policy = CacheWritePolicy::Additional;
    assert_eq!(input.validate().unwrap().value(&writes), Ok(230_000_000));
    input.reasoning_policy = ReasoningPolicy::Unknown;
    assert_eq!(
        input.validate().unwrap().value(&writes),
        Err(Error::UnknownReasoning)
    );
    input.reasoning_policy = ReasoningPolicy::Included;
    let mut missing = writes.clone();
    missing.total_tokens = Counter::Missing;
    assert_eq!(
        input.validate().unwrap().value(&missing),
        Err(Error::MissingCategories)
    );
    for invalid in [
        [100, 101, 0, 40, 10, 140],
        [100, 20, 0, 40, 41, 140],
        [-1, 0, 0, 0, 0, 0],
    ] {
        assert_eq!(
            input.validate().unwrap().value(&tokens(invalid)),
            Err(Error::InvalidCategories)
        );
    }
    input.cache_write_policy = CacheWritePolicy::IncludedInputDisjoint;
    assert_eq!(
        input
            .validate()
            .unwrap()
            .value(&tokens([100, 20, 81, 40, 10, 140])),
        Err(Error::InvalidCategories)
    );
    input = prices();
    input.input = "0".into();
    input.cached_input = "0".into();
    input.output = "0".into();
    assert_eq!(
        input
            .validate()
            .unwrap()
            .value(&tokens([100, 20, 0, 40, 10, 140])),
        Ok(0)
    );
    input.cache_write_policy = CacheWritePolicy::Additional;
    input.input = "9223372036854.775807".into();
    input.output = input.input.clone();
    input.cache_write = input.input.clone();
    assert_eq!(
        input
            .validate()
            .unwrap()
            .value(&tokens([i64::MAX, 0, i64::MAX, i64::MAX, 0, i64::MAX])),
        Err(Error::Overflow)
    );
}

#[test]
fn pricing_source_boundaries_edits_and_no_implicit_backfill() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("pricing.sqlite")).unwrap();
    context(&mut store, "a", "thread", Some("model"));
    let old = usage(
        &mut store,
        "a",
        "thread",
        1,
        "2026-01-01T00:00:00.999999999Z",
    );
    let first = store
        .save_model_price_at("model", prices(), false, time("2026-01-01T00:00:01Z"))
        .unwrap();
    let boundary = usage(&mut store, "a", "thread", 2, "2026-01-01T02:00:01+02:00");
    let mut changed = prices();
    changed.input = "2".into();
    let second = store
        .save_model_price_at("model", changed, false, time("2026-01-01T00:00:02Z"))
        .unwrap();
    let after = usage(&mut store, "a", "thread", 3, "2026-01-01T00:00:02Z");
    drain(&mut store);
    assert!(store.observation_valuation(old).unwrap().is_none());
    let value = store.observation_valuation(boundary).unwrap().unwrap();
    assert_eq!(
        (value.version_id, value.amount.as_str()),
        (first.id, "210000000")
    );
    let value = store.observation_valuation(after).unwrap().unwrap();
    assert_eq!(
        (value.version_id, value.amount.as_str()),
        (second.id, "290000000")
    );
    assert!(store
        .save_model_price_at("model", prices(), false, time("2026-01-01T00:00:02Z"))
        .is_err());
    assert!(store
        .save_model_price_at("model", prices(), false, time("2025-01-01T00:00:02Z"))
        .is_err());
    assert!(store
        .save_model_price_at("model", prices(), true, time("2026-01-01T00:00:03Z"))
        .is_err());
    assert_eq!(super::totals(&store), (420, 0, 3));
}

#[test]
fn pricing_initial_backfill_is_bounded_durable_and_immutable_on_reimport() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("pricing.sqlite");
    let mut store = Store::open(&path).unwrap();
    context(&mut store, "a", "thread", Some("later-model"));
    for seq in 1..=150 {
        usage(
            &mut store,
            "a",
            "thread",
            seq,
            &format!("2026-01-01T00:{:02}:{:02}Z", seq / 60, seq % 60),
        );
    }
    let version = store
        .save_model_price_at("later-model", prices(), true, time("2026-02-01T00:00:00Z"))
        .unwrap();
    assert_eq!(store.process_pricing_work().unwrap(), PRICING_BATCH_SIZE);
    assert_eq!(
        store
            .connection()
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM observation_valuations", [], |r| r
                .get(0))
            .unwrap(),
        64
    );
    drop(store);
    let mut store = Store::open(&path).unwrap();
    assert!(store.pricing_work_pending().unwrap());
    drain(&mut store);
    let before = store.observation_valuation(1).unwrap().unwrap();
    assert_eq!(
        (before.version_id, before.amount.as_str()),
        (version.id, "210000000")
    );
    let mut changed = prices();
    changed.input = "99".into();
    store
        .save_model_price_at("later-model", changed, false, time("2026-03-01T00:00:00Z"))
        .unwrap();
    drain(&mut store);
    context(&mut store, "reimport", "thread", Some("later-model"));
    for seq in 1..=150 {
        usage(
            &mut store,
            "reimport",
            "thread",
            seq,
            &format!("2026-01-01T00:{:02}:{:02}Z", seq / 60, seq % 60),
        );
    }
    assert_eq!(store.observation_valuation(1).unwrap(), Some(before));
    assert_eq!(super::totals(&store), (21000, 0, 150));
    assert_eq!(
        store
            .connection()
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM observation_valuations", [], |r| r
                .get(0))
            .unwrap(),
        150
    );
    assert_eq!(store.process_pricing_work().unwrap(), 0);
}

#[test]
fn pricing_model_conflict_preserves_value_and_detected_names() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("pricing.sqlite")).unwrap();
    context(&mut store, "a", "thread", Some("first"));
    store
        .save_model_price_at("first", prices(), false, time("2026-01-01T00:00:00Z"))
        .unwrap();
    let id = usage(&mut store, "a", "thread", 1, "2026-01-01T00:00:01Z");
    record_in_store(
        &mut store,
        "a",
        &json!({"type":"turn_context","payload":{"turn_id":"turn","model":"conflicting"}}),
    );
    let value = store.observation_valuation(id).unwrap().unwrap();
    assert_eq!(value.model, "first");
    assert_eq!(value.amount, "210000000");
    assert!(value.attribution_conflict);
    let unpriced = usage(&mut store, "a", "thread", 2, "2026-01-01T00:00:02Z");
    store
        .save_model_price_at("conflicting", prices(), true, time("2026-01-01T00:00:03Z"))
        .unwrap();
    drain(&mut store);
    assert!(store.observation_valuation(unpriced).unwrap().is_none());
    assert_eq!(
        store
            .pricing_models(None)
            .unwrap()
            .iter()
            .map(|m| m.model.as_str())
            .collect::<Vec<_>>(),
        vec!["conflicting", "first"]
    );
    assert!(store.save_model_price("unknown", prices(), true).is_err());
    assert_eq!(super::totals(&store), (280, 0, 2));
}

#[test]
fn pricing_work_failure_rolls_back_cursor_and_valuations() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("pricing.sqlite");
    let mut store = Store::open(&path).unwrap();
    context(&mut store, "a", "thread", Some("model"));
    usage(&mut store, "a", "thread", 1, "2026-01-01T00:00:01Z");
    usage(&mut store, "a", "thread", 2, "2026-01-01T00:00:02Z");
    store
        .save_model_price_at("model", prices(), true, time("2026-01-01T00:00:03Z"))
        .unwrap();
    store.connection().execute_batch("CREATE TRIGGER fail_pricing BEFORE INSERT ON observation_valuations WHEN NEW.observation_id=2 BEGIN SELECT RAISE(ABORT,'simulated interruption'); END;").unwrap();
    assert!(store.process_pricing_work().is_err());
    assert!(store.observation_valuation(1).unwrap().is_none());
    assert_eq!(
        store
            .connection()
            .query_row::<i64, _, _>("SELECT after_id FROM pricing_work", [], |r| r.get(0))
            .unwrap(),
        0
    );
    store
        .connection()
        .execute_batch("DROP TRIGGER fail_pricing;")
        .unwrap();
    drop(store);
    let mut store = Store::open(&path).unwrap();
    drain(&mut store);
    assert_eq!(
        store.observation_valuation(1).unwrap().unwrap().amount,
        "210000000"
    );
    assert_eq!(
        store.observation_valuation(2).unwrap().unwrap().amount,
        "210000000"
    );
}

#[test]
fn pricing_delayed_import_and_future_clock_skew_keep_source_time_and_existing_value() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("pricing.sqlite")).unwrap();
    context(&mut store, "future", "future", Some("model"));
    let first = store
        .save_model_price_at("model", prices(), false, time("2026-01-01T00:00:01Z"))
        .unwrap();
    let future = usage(&mut store, "future", "future", 1, "2099-01-01T00:00:00Z");
    let before = store.observation_valuation(future).unwrap().unwrap();
    let mut changed = prices();
    changed.input = "9".into();
    store
        .save_model_price_at("model", changed, false, time("2026-01-01T00:00:02Z"))
        .unwrap();
    context(&mut store, "delayed", "delayed", Some("model"));
    let delayed = usage(
        &mut store,
        "delayed",
        "delayed",
        1,
        "2026-01-01T00:00:01.999999999Z",
    );
    context(&mut store, "old", "old", Some("model"));
    let old = usage(&mut store, "old", "old", 1, "2025-01-01T00:00:00Z");
    drain(&mut store);
    assert_eq!(store.observation_valuation(future).unwrap(), Some(before));
    assert_eq!(
        store
            .observation_valuation(delayed)
            .unwrap()
            .unwrap()
            .version_id,
        first.id
    );
    assert!(store.observation_valuation(old).unwrap().is_none());
}

#[test]
fn pricing_pending_usage_waits_for_acceptance_and_catalog_pages_are_bounded() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("pricing.sqlite")).unwrap();
    context(&mut store, "a", "thread", Some("model"));
    store
        .save_model_price_at("model", prices(), false, time("2026-01-01T00:00:00Z"))
        .unwrap();
    usage(&mut store, "a", "thread", 1, "2026-01-01T00:00:01Z");
    let pending = usage(&mut store, "a", "thread", 3, "2026-01-01T00:00:03Z");
    drain(&mut store);
    assert!(store.observation_valuation(pending).unwrap().is_none());
    usage(&mut store, "a", "thread", 2, "2026-01-01T00:00:02Z");
    super::settle(&mut store);
    assert_eq!(
        store
            .observation_valuation(pending)
            .unwrap()
            .unwrap()
            .amount,
        "210000000"
    );
    assert_eq!(super::totals(&store), (420, 0, 3));
    for n in 0..70 {
        record_in_store(
            &mut store,
            "catalog",
            &json!({"type":"turn_context","payload":{"turn_id":"turn","model":format!("catalog-{n:02}")}}),
        );
    }
    let first = store.pricing_models(None).unwrap();
    assert_eq!(first.len(), 64);
    let second = store
        .pricing_models(Some(&first.last().unwrap().model))
        .unwrap();
    assert_eq!(second.len(), 7);
    assert!(store
        .pricing_models(Some(&second.last().unwrap().model))
        .unwrap()
        .is_empty());
}

#[test]
fn pricing_v6_migration_seeds_surviving_names_and_unknown_is_separate() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("v6.sqlite");
    let db = rusqlite::Connection::open(&path).unwrap();
    for sql in [
        include_str!("../../migrations/001_initial.sql"),
        include_str!("../../migrations/002_resumable_sources.sql"),
        include_str!("../../migrations/003_snapshot_chronology.sql"),
        include_str!("../../migrations/004_metadata_evidence.sql"),
        include_str!("../../migrations/005_identity_resolution.sql"),
        include_str!("../../migrations/006_aggregate_queries.sql"),
    ] {
        db.execute_batch(sql).unwrap();
    }
    db.execute_batch("INSERT INTO sessions(thread_id) VALUES('thread'); INSERT INTO turn_contexts VALUES('thread','one','surviving'),('thread','two',NULL);").unwrap();
    db.execute_batch("INSERT INTO observations(thread_id,endpoint,timestamp,normalized,adapter,source_path,source_offset,source_ordinal,model,accepted) VALUES('thread','{}','2026-01-01T00:00:00Z','{}','modern-1','old',0,1,'usage-only',0);").unwrap();
    drop(db);
    let mut store = Store::open(&path).unwrap();
    assert_eq!(store.pricing_models(None).unwrap()[0].model, "surviving");
    assert_eq!(
        store.pricing_models(Some("surviving")).unwrap()[0].model,
        "usage-only"
    );
    assert!(!store.pricing_work_pending().unwrap());
    context(&mut store, "a", "unknown", None);
    let id = usage(&mut store, "a", "unknown", 1, "2026-01-01T00:00:00Z");
    assert!(store.observation_valuation(id).unwrap().is_none());
    assert_eq!(store.pricing_models(None).unwrap().len(), 2);
}
