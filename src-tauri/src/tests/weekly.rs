use super::record_in_store;
use crate::{
    adapter,
    pricing::{CacheWritePolicy, PriceInput, ReasoningPolicy},
    storage::Store,
    weekly::{self, Query, Time, Unavailable},
};
use serde_json::json;

fn time(value: &str) -> Time {
    let (seconds, nanos) = adapter::observation_time(value).unwrap();
    Time { seconds, nanos }
}
fn query(store: &mut Store, now: &str) -> weekly::Response {
    store
        .weekly_at(
            Query {
                before: None,
                limit: 50,
            },
            time(now),
        )
        .unwrap()
}
pub(super) fn limit(
    store: &mut Store,
    timestamp: &str,
    percent: &str,
    bucket: &str,
    minutes: u32,
    position: &str,
    reset: Option<i64>,
) {
    let value: serde_json::Number = serde_json::from_str(percent).unwrap();
    record_in_store(
        store,
        "limits",
        &json!({"type":"event_msg","timestamp":timestamp,"payload":{"type":"token_count","rate_limits":{"limit_id":bucket,position:{"used_percent":value,"window_minutes":minutes,"resets_at":reset}}}}),
    );
}
fn weekly(store: &mut Store, timestamp: &str, percent: &str) {
    window(store, timestamp, percent, 0);
}
/// A sample in weekly window `index`. A reset is a new window with its own
/// reset time, exactly as the source reports one, not merely a lower percentage.
pub(super) fn window(store: &mut Store, timestamp: &str, percent: &str, index: i64) {
    limit(
        store,
        timestamp,
        percent,
        "codex",
        10080,
        "secondary",
        Some(2_000_000_000 + index * 604_800),
    );
}
fn usage(store: &mut Store, thread: &str, timestamp: &str, model: &str) {
    record_in_store(
        store,
        thread,
        &json!({"type":"session_meta","payload":{"id":thread}}),
    );
    record_in_store(
        store,
        thread,
        &json!({"type":"turn_context","payload":{"turn_id":"turn","model":model}}),
    );
    let tokens = json!({"input_tokens":1000000,"cached_input_tokens":0,"cache_write_input_tokens":0,"output_tokens":0,"reasoning_output_tokens":0,"total_tokens":1000000});
    record_in_store(
        store,
        thread,
        &json!({"type":"token_usage_record","timestamp":timestamp,"payload":{"thread_id":thread,"turn_id":"turn","response_id":"response","usage":tokens,"thread_token_usage":tokens}}),
    );
}
fn price(store: &mut Store) {
    store
        .save_model_price_at(
            "priced",
            PriceInput {
                input: "1".into(),
                cached_input: "1".into(),
                cache_write: "1".into(),
                output: "1".into(),
                reasoning: None,
                reasoning_policy: ReasoningPolicy::Included,
                cache_write_policy: CacheWritePolicy::Unknown,
            },
            true,
            (0, 0),
        )
        .unwrap();
    while store.pricing_work_pending().unwrap() {
        store.process_pricing_work().unwrap();
    }
}

#[test]
fn weekly_chronological_distinct_duplicate_history_restart_and_reimport() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("weekly.sqlite");
    let mut store = Store::open(&db).unwrap();
    // Deliberately ingest a reset before its older high-water observations.
    window(&mut store, "2026-01-01T00:20:00Z", "2", 1);
    weekly(&mut store, "2026-01-01T00:00:00Z", "40");
    weekly(&mut store, "2026-01-01T00:10:00Z", "42");
    limit(
        &mut store,
        "2026-01-01T00:10:00Z",
        "42",
        "codex",
        10080,
        "primary",
        Some(2_000_000_000),
    );
    limit(
        &mut store,
        "2026-01-01T00:15:00Z",
        "1",
        "codex",
        300,
        "primary",
        None,
    );
    limit(
        &mut store,
        "2026-01-01T00:15:00Z",
        "1",
        "spark",
        10080,
        "secondary",
        None,
    );
    let first = query(&mut store, "2026-01-01T00:30:00Z");
    assert_eq!(first.history.len(), 1);
    assert_eq!(first.history[0].cycle.last_observation.used_percent, "42");
    let current = first.current_cycle.unwrap();
    assert!(current.detected_reset);
    assert_eq!(current.first_observation.time, time("2026-01-01T00:20:00Z"));
    assert_eq!(current.last_observation.remaining_percent, "98");
    assert_eq!(current.last_observation.resets_at, Some(2_000_604_800));
    assert!(!current.full_cycle_cost_known);
    assert!(first.session_weekly_percentage_impact.is_none());
    let rows: i64 = store
        .connection()
        .query_row("SELECT COUNT(*) FROM limit_samples", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        rows, 5,
        "alternate windows remain metadata, duplicate position does not add a sample"
    );
    drop(store);
    let mut store = Store::open(&db).unwrap();
    weekly(&mut store, "2026-01-01T00:00:00Z", "40");
    window(&mut store, "2026-01-01T00:20:00Z", "2", 1);
    let restarted = query(&mut store, "2026-01-01T00:30:00Z");
    assert_eq!(restarted.history.len(), 1);
    assert_eq!(restarted.current_cycle.unwrap().key, current.key);
    assert_eq!(restarted.history[0].cycle.key, first.history[0].cycle.key);
}

#[test]
fn weekly_partial_interval_stale_alignment_recent_expiry_and_immutable_cost() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("cost.sqlite")).unwrap();
    weekly(&mut store, "2026-01-01T00:00:00Z", "40");
    weekly(&mut store, "2026-01-01T00:05:00Z", "41");
    weekly(&mut store, "2026-01-01T00:10:00Z", "42");
    usage(&mut store, "at-start", "2026-01-01T00:00:00Z", "unpriced");
    usage(&mut store, "inside", "2026-01-01T00:04:00Z", "priced");
    usage(&mut store, "at-end", "2026-01-01T00:10:00Z", "priced");
    usage(&mut store, "newer", "2026-01-01T00:11:00Z", "unpriced");
    price(&mut store);
    let response = query(&mut store, "2026-01-01T00:15:00Z");
    assert!(response.coverage_note.contains("Since observation began"));
    assert_eq!(
        response.overall.consumed_percentage_points.as_deref(),
        Some("2")
    );
    assert_eq!(
        response
            .overall
            .estimated_cost
            .as_ref()
            .unwrap()
            .known_subtotal
            .as_deref(),
        Some("2000000000000")
    );
    assert_eq!(
        response.overall.effective_usd_per_percent.as_deref(),
        Some("1.000000000000")
    );
    assert_eq!(
        response.overall.estimated_full_week_usd.as_deref(),
        Some("100.000000000000")
    );
    assert!(response.recent.unavailable_reason.is_none());
    assert_eq!(response.observation_age_seconds, Some(300));
    assert_eq!(
        response.unmatched_cost_start,
        Some(time("2026-01-01T00:10:00Z"))
    );
    assert!(!response.unmatched_cost.unwrap().complete);
    // At 00:16 the 00:00 endpoint falls outside the 15-minute window.
    let recent = query(&mut store, "2026-01-01T00:16:00Z").recent;
    assert_eq!(recent.start, Some(time("2026-01-01T00:05:00Z")));
    assert_eq!(
        recent.estimated_cost.unwrap().known_subtotal.as_deref(),
        Some("1000000000000")
    );
    assert_eq!(
        recent.effective_usd_per_percent.as_deref(),
        Some("1.000000000000")
    );
    let expired = query(&mut store, "2026-01-01T00:25:00.000000001Z");
    assert_eq!(
        expired.recent.unavailable_reason,
        Some(Unavailable::InsufficientObservations)
    );
    assert_eq!(
        expired.overall.effective_usd_per_percent.as_deref(),
        Some("1.000000000000")
    );
    // A later configured price cannot change an already matched historical ratio.
    store
        .save_model_price_at(
            "priced",
            PriceInput {
                input: "9".into(),
                cached_input: "9".into(),
                cache_write: "9".into(),
                output: "9".into(),
                reasoning: None,
                reasoning_policy: ReasoningPolicy::Included,
                cache_write_policy: CacheWritePolicy::Unknown,
            },
            false,
            (2_000_000_000, 0),
        )
        .unwrap();
    while store.pricing_work_pending().unwrap() {
        store.process_pricing_work().unwrap();
    }
    assert_eq!(
        query(&mut store, "2026-01-01T00:25:00Z")
            .overall
            .effective_usd_per_percent
            .as_deref(),
        Some("1.000000000000")
    );
}

#[test]
fn weekly_exact_one_point_boundary_unpriced_suppression_and_zero_cost() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("boundary.sqlite")).unwrap();
    weekly(&mut store, "2026-01-01T00:00:00Z", "10");
    weekly(
        &mut store,
        "2026-01-01T00:01:00Z",
        "10.999999999999999999999999999999",
    );
    let small = query(&mut store, "2026-01-01T00:01:00Z");
    assert_eq!(
        small.overall.unavailable_reason,
        Some(Unavailable::BelowOnePercentagePoint)
    );
    weekly(&mut store, "2026-01-01T00:02:00Z", "11");
    let exact = query(&mut store, "2026-01-01T00:02:00Z");
    assert_eq!(
        exact.overall.effective_usd_per_percent.as_deref(),
        Some("0.000000000000")
    );
    usage(
        &mut store,
        "missing-price",
        "2026-01-01T00:01:30Z",
        "unpriced",
    );
    let unpriced = query(&mut store, "2026-01-01T00:02:00Z");
    assert_eq!(
        unpriced.overall.unavailable_reason,
        Some(Unavailable::UnpricedUsage)
    );
    assert_eq!(
        unpriced.recent.unavailable_reason,
        Some(Unavailable::UnpricedUsage)
    );
    let cost = unpriced.overall.estimated_cost.unwrap();
    assert!(!cost.complete);
    assert!(cost.known_subtotal.is_none());
}

#[test]
fn weekly_lagging_sessions_re_report_earlier_snapshots_without_restarting_the_cycle() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("stale.sqlite")).unwrap();
    // Concurrent sessions carry snapshots captured moments apart, so each
    // percentage tick arrives interleaved with the value it replaced.
    for (timestamp, percent) in [
        ("2026-01-01T00:00:00Z", "20"),
        ("2026-01-01T00:01:00Z", "21"),
        ("2026-01-01T00:01:10Z", "20"),
        ("2026-01-01T00:02:00Z", "22"),
        ("2026-01-01T00:02:10Z", "21"),
        ("2026-01-01T00:03:00Z", "23"),
    ] {
        weekly(&mut store, timestamp, percent);
    }
    usage(&mut store, "priced", "2026-01-01T00:02:30Z", "priced");
    price(&mut store);
    let response = query(&mut store, "2026-01-01T00:03:00Z");
    assert_eq!(response.stale_samples, 2);
    assert!(
        response.history.is_empty(),
        "a lower percentage within one window is not a reset"
    );
    let cycle = response.current_cycle.unwrap();
    assert!(!cycle.detected_reset);
    assert_eq!(cycle.first_observation.time, time("2026-01-01T00:00:00Z"));
    assert_eq!(cycle.last_observation.used_percent, "23");
    // The comparable interval keeps its original baseline, so cost accumulates
    // across the ticks instead of restarting at each re-reported snapshot.
    assert_eq!(response.overall.start, Some(time("2026-01-01T00:00:00Z")));
    assert_eq!(
        response.overall.consumed_percentage_points.as_deref(),
        Some("3")
    );
    assert_eq!(
        response
            .overall
            .estimated_cost
            .unwrap()
            .known_subtotal
            .as_deref(),
        Some("1000000000000")
    );
}

#[test]
fn weekly_window_metadata_decides_resets_and_retires_superseded_samples() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("windows.sqlite")).unwrap();
    weekly(&mut store, "2026-01-01T00:00:00Z", "90");
    weekly(&mut store, "2026-01-01T00:01:00Z", "95");
    // A new window is a reset even though its first percentage is not lower.
    window(&mut store, "2026-01-01T00:02:00Z", "96", 1);
    // A session that had not yet seen the new window reports the old one late.
    weekly(&mut store, "2026-01-01T00:02:30Z", "95");
    window(&mut store, "2026-01-01T00:03:00Z", "97", 1);
    let response = query(&mut store, "2026-01-01T00:04:00Z");
    assert_eq!(response.stale_samples, 1);
    assert_eq!(response.history.len(), 1);
    assert_eq!(
        response.history[0].cycle.last_observation.time,
        time("2026-01-01T00:01:00Z")
    );
    let cycle = response.current_cycle.unwrap();
    assert!(cycle.detected_reset);
    assert_eq!(cycle.first_observation.time, time("2026-01-01T00:02:00Z"));
    assert_eq!(cycle.last_observation.resets_at, Some(2_000_604_800));
    assert_eq!(
        response.overall.consumed_percentage_points.as_deref(),
        Some("1")
    );
    // Reset times jitter by seconds within one window without splitting it.
    limit(
        &mut store,
        "2026-01-01T00:05:00Z",
        "98",
        "codex",
        10080,
        "secondary",
        Some(2_000_604_812),
    );
    let jitter = query(&mut store, "2026-01-01T00:06:00Z");
    assert_eq!(jitter.history.len(), 1);
    assert_eq!(
        jitter.current_cycle.unwrap().first_observation.time,
        time("2026-01-01T00:02:00Z")
    );
}

#[test]
fn weekly_ties_barriers_reset_metadata_and_untrustworthy_samples() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("ties.sqlite")).unwrap();
    weekly(&mut store, "2026-01-01T00:00:00Z", "40");
    limit(
        &mut store,
        "2026-01-01T01:00:00+01:00",
        "4e1",
        "codex",
        10080,
        "primary",
        None,
    );
    let tied = query(&mut store, "2026-01-01T00:00:00Z");
    assert!(tied
        .current_cycle
        .unwrap()
        .last_observation
        .resets_at
        .is_none());
    assert_eq!(
        tied.overall.unavailable_reason,
        Some(Unavailable::InsufficientObservations)
    );
    weekly(&mut store, "2026-01-01T00:01:00Z", "42");
    weekly(&mut store, "2026-01-01T00:01:00Z", "1");
    let barrier = query(&mut store, "2026-01-01T00:01:00Z");
    assert_eq!(
        barrier.overall.unavailable_reason,
        Some(Unavailable::AmbiguousObservation)
    );
    assert_eq!(
        barrier.current_cycle.unwrap().last_observation.used_percent,
        "40"
    );
    weekly(&mut store, "2026-01-01T00:02:00Z", "42");
    weekly(&mut store, "2026-01-01T00:03:00Z", "43");
    weekly(&mut store, "not-a-time", "0");
    weekly(&mut store, "2026-01-01T00:04:00Z", "101");
    weekly(&mut store, "2026-01-01T00:10:00Z", "0");
    let recovered = query(&mut store, "2026-01-01T00:05:00Z");
    assert_eq!(recovered.excluded_samples, 3);
    assert!(
        recovered.history.is_empty(),
        "an ambiguous decrease does not assert a cycle reset"
    );
    assert!(recovered.current_cycle.unwrap().has_ambiguous_observations);
    assert_eq!(recovered.overall.start, Some(time("2026-01-01T00:02:00Z")));
    assert_eq!(
        recovered.overall.consumed_percentage_points.as_deref(),
        Some("1")
    );
    assert!(weekly::percentage("1e-1025").is_none());
    assert!(weekly::percentage("1e99999999999999999999").is_none());
}

#[test]
fn weekly_reset_excludes_prior_cost_and_history_pages_are_bounded() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("pages.sqlite")).unwrap();
    weekly(&mut store, "2026-01-01T00:00:00Z", "50");
    window(&mut store, "2026-01-01T00:01:00Z", "1", 1);
    window(&mut store, "2026-01-01T00:02:00Z", "2", 1);
    window(&mut store, "2026-01-01T00:03:00Z", "0", 2);
    window(&mut store, "2026-01-01T00:04:00Z", "1", 2);
    usage(&mut store, "old", "2026-01-01T00:03:00Z", "unpriced");
    let first = store
        .weekly_at(
            Query {
                before: None,
                limit: 1,
            },
            time("2026-01-01T00:04:00Z"),
        )
        .unwrap();
    assert_eq!(
        first.overall.effective_usd_per_percent.as_deref(),
        Some("0.000000000000")
    );
    assert_eq!(first.history.len(), 1);
    assert_eq!(
        first.history[0].cycle.first_observation.time,
        time("2026-01-01T00:01:00Z")
    );
    let second = store
        .weekly_at(
            Query {
                before: first.next_cursor,
                limit: 1,
            },
            time("2026-01-01T00:04:00Z"),
        )
        .unwrap();
    assert_eq!(second.history.len(), 1);
    assert_eq!(
        second.history[0].cycle.first_observation.time,
        time("2026-01-01T00:00:00Z")
    );
    assert!(second.next_cursor.is_none());
    assert_eq!(
        store
            .weekly_at(
                Query {
                    before: Some("invalid".into()),
                    limit: 1
                },
                time("2026-01-01T00:04:00Z")
            )
            .unwrap_err(),
        weekly::ReadError::InvalidQuery
    );
    assert_eq!(
        store
            .weekly_at(
                Query {
                    before: None,
                    limit: 51
                },
                time("2026-01-01T00:04:00Z")
            )
            .unwrap_err(),
        weekly::ReadError::InvalidQuery
    );
}

#[test]
fn weekly_ratio_rounds_exact_rational_once_and_reset_comparison_is_exact() {
    use weekly::{Cost, Estimate, Sample};
    let start = Sample {
        time: time("2026-01-01T00:00:00Z"),
        used: weekly::percentage("0").unwrap(),
        reset: None,
    };
    let estimate = |denominator: &str, amount: &str| {
        let end = Sample {
            time: time("2026-01-01T00:01:00Z"),
            used: weekly::percentage(denominator).unwrap(),
            reset: None,
        };
        Estimate::matched(
            &start,
            &end,
            Cost {
                known_subtotal: Some(amount.into()),
                complete: true,
                accepted_observations: 1,
            },
        )
    };
    // Half a trillionth rounds to even zero; 1.5 rounds to even two.
    assert_eq!(
        estimate("2", "1").effective_usd_per_percent.as_deref(),
        Some("0.000000000000")
    );
    assert_eq!(
        estimate("2", "3").effective_usd_per_percent.as_deref(),
        Some("0.000000000002")
    );
    // More than 100 fractional digits avoids a decimal intermediate double-round.
    let just_below_two = format!("1.{}", "9".repeat(150));
    assert_eq!(
        estimate(&just_below_two, "1")
            .effective_usd_per_percent
            .as_deref(),
        Some("0.000000000001")
    );
    assert_eq!(
        estimate("3", "1000000000000")
            .estimated_full_week_usd
            .as_deref(),
        Some("33.333333333333")
    );
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("exact-reset.sqlite")).unwrap();
    weekly(
        &mut store,
        "2026-01-01T00:00:00Z",
        "1.000000000000000000000000000001",
    );
    weekly(&mut store, "2026-01-01T00:01:00Z", "1");
    // The exact comparison recognizes the smaller value; within one window it is
    // an earlier snapshot re-reported, so the high-water percentage stands.
    let exact = query(&mut store, "2026-01-01T00:02:00Z");
    assert_eq!(exact.stale_samples, 1);
    let cycle = exact.current_cycle.unwrap();
    assert!(!cycle.detected_reset);
    assert_eq!(
        cycle.last_observation.used_percent,
        "1.000000000000000000000000000001"
    );
    window(&mut store, "2026-01-01T00:03:00Z", "1", 1);
    assert!(
        query(&mut store, "2026-01-01T00:04:00Z")
            .current_cycle
            .unwrap()
            .detected_reset
    );
}

#[test]
fn weekly_read_only_delivery_preserves_storage_and_reports_bad_cost() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("read.sqlite");
    let mut store = Store::open(&db).unwrap();
    weekly(&mut store, "2026-01-01T00:00:00Z", "1");
    weekly(&mut store, "2026-01-01T00:01:00Z", "2");
    usage(&mut store, "priced", "2026-01-01T00:00:30Z", "priced");
    price(&mut store);
    drop(store);
    let response = Store::read_weekly(
        &db,
        Query {
            before: None,
            limit: 1,
        },
    )
    .unwrap();
    assert_eq!(
        response.overall.effective_usd_per_percent.as_deref(),
        Some("1.000000000000")
    );
    assert_eq!(
        response.recent.unavailable_reason,
        Some(Unavailable::InsufficientObservations)
    );
    let mut store = Store::open(&db).unwrap();
    // The exact-cost accumulator rejects corrupt valuation text rather than
    // presenting a rounded or fabricated ratio through the delivery boundary.
    // Bypass the write guard only in this disposable corruption fixture.
    store
        .connection()
        .execute_batch("DROP TRIGGER immutable_valuation_update")
        .unwrap();
    store
        .connection()
        .execute("UPDATE observation_valuations SET amount='not-money'", [])
        .unwrap();
    assert_eq!(
        store
            .weekly_at(
                Query {
                    before: None,
                    limit: 1
                },
                time("2026-01-01T00:01:00Z")
            )
            .unwrap_err(),
        weekly::ReadError::Storage
    );
    let missing = temp.path().join("missing.sqlite");
    assert_eq!(
        Store::read_weekly(
            &missing,
            Query {
                before: None,
                limit: 1
            }
        )
        .unwrap_err(),
        weekly::ReadError::Storage
    );
    assert!(!missing.exists());
}

fn models(
    store: &mut Store,
    key: &str,
    after: Option<String>,
    limit: u32,
) -> Option<weekly::Models> {
    store
        .weekly_models_at(
            weekly::ModelsQuery {
                cycle_key: key.into(),
                page: crate::aggregates::PageRequest { after, limit },
            },
            time("2026-01-02T00:00:00Z"),
        )
        .unwrap()
}

#[test]
fn weekly_history_matches_pre_reset_interval_models_and_preserves_priced_replay() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("history.sqlite");
    let mut store = Store::open(&db).unwrap();
    weekly(&mut store, "2026-01-01T00:00:00Z", "40");
    weekly(&mut store, "2026-01-01T00:10:00Z", "42");
    usage(&mut store, "start", "2026-01-01T00:00:00Z", "unpriced");
    usage(&mut store, "inside", "2026-01-01T00:05:00Z", "priced");
    usage(&mut store, "end", "2026-01-01T00:10:00Z", "priced");
    usage(
        &mut store,
        "after",
        "2026-01-01T00:10:00.000000001Z",
        "unpriced",
    );
    price(&mut store);
    let before = query(&mut store, "2026-01-01T00:15:00Z");
    window(&mut store, "2026-01-01T00:20:00Z", "1", 1);
    let result = query(&mut store, "2026-01-01T00:20:00Z");
    let history = &result.history[0];
    assert_eq!(
        serde_json::to_value(&history.estimate).unwrap(),
        serde_json::to_value(before.overall).unwrap()
    );
    assert_eq!(
        history.estimate.consumed_percentage_points.as_deref(),
        Some("2")
    );
    assert_eq!(
        history.estimate.effective_usd_per_percent.as_deref(),
        Some("1.000000000000")
    );
    assert_eq!(
        history.estimate.estimated_full_week_usd.as_deref(),
        Some("100.000000000000")
    );
    assert_eq!(
        history
            .tokens
            .as_ref()
            .unwrap()
            .total_tokens
            .known_tokens
            .as_deref(),
        Some("2000000")
    );
    assert!(history.tokens.as_ref().unwrap().input_tokens.complete);
    assert!(!history.cycle.detected_reset);
    assert!(!history.cycle.full_cycle_cost_known);
    let key = history.cycle.key.clone();
    let breakdown = models(&mut store, &key, None, 1).unwrap();
    assert_eq!(breakdown.items.len(), 1);
    assert_eq!(breakdown.items[0].id, "model:priced");
    assert_eq!(
        breakdown.items[0]
            .tokens
            .total_tokens
            .known_tokens
            .as_deref(),
        Some("2000000")
    );
    assert_eq!(
        breakdown.items[0].estimated_cost.known_subtotal.as_deref(),
        Some("2000000000000")
    );
    assert!(breakdown.next_cursor.is_none());
    let original = serde_json::to_value(history).unwrap();
    assert_eq!(original["key"], key);
    assert!(
        original.get("cycle").is_none(),
        "Cycle fields remain flattened on the wire"
    );
    store
        .save_model_price_at(
            "priced",
            PriceInput {
                input: "9".into(),
                cached_input: "9".into(),
                cache_write: "9".into(),
                output: "9".into(),
                reasoning: None,
                reasoning_policy: ReasoningPolicy::Included,
                cache_write_policy: CacheWritePolicy::Unknown,
            },
            false,
            (2_000_000_000, 0),
        )
        .unwrap();
    while store.pricing_work_pending().unwrap() {
        store.process_pricing_work().unwrap();
    }
    drop(store);
    let mut store = Store::open(&db).unwrap();
    usage(&mut store, "inside", "2026-01-01T00:05:00Z", "priced");
    weekly(&mut store, "2026-01-01T00:00:00Z", "40");
    window(&mut store, "2026-01-01T00:20:00Z", "1", 1);
    assert_eq!(
        serde_json::to_value(&query(&mut store, "2026-01-01T00:20:00Z").history[0]).unwrap(),
        original
    );
    let reopened = Store::read_weekly_models(
        &db,
        weekly::ModelsQuery {
            cycle_key: key,
            page: crate::aggregates::PageRequest {
                after: None,
                limit: 1,
            },
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        serde_json::to_value(reopened).unwrap(),
        serde_json::to_value(breakdown).unwrap()
    );
}

#[test]
fn weekly_history_recovers_comparable_segment_and_preserves_unavailability() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("segments.sqlite")).unwrap();
    weekly(&mut store, "2026-01-01T00:00:00Z", "40");
    weekly(&mut store, "2026-01-01T00:01:00Z", "41");
    weekly(&mut store, "2026-01-01T00:01:00Z", "1");
    assert_eq!(
        query(&mut store, "2026-01-01T00:01:00Z")
            .overall
            .unavailable_reason,
        Some(Unavailable::AmbiguousObservation)
    );
    weekly(&mut store, "2026-01-01T00:02:00Z", "42");
    weekly(&mut store, "2026-01-01T00:03:00Z", "43");
    usage(
        &mut store,
        "before-recovery",
        "2026-01-01T00:01:30Z",
        "unpriced",
    );
    usage(&mut store, "recovered", "2026-01-01T00:03:00Z", "priced");
    price(&mut store);
    let recovered = query(&mut store, "2026-01-01T00:03:00Z");
    window(&mut store, "2026-01-01T00:04:00Z", "0", 1);
    let complete = query(&mut store, "2026-01-01T00:04:00Z");
    let history = &complete.history[0];
    assert!(history.cycle.has_ambiguous_observations);
    assert_eq!(
        history.cycle.first_observation.time,
        time("2026-01-01T00:00:00Z")
    );
    assert_eq!(history.estimate.start, Some(time("2026-01-01T00:02:00Z")));
    assert_eq!(
        history.estimate.consumed_percentage_points.as_deref(),
        Some("1")
    );
    assert_eq!(
        history
            .tokens
            .as_ref()
            .unwrap()
            .total_tokens
            .known_tokens
            .as_deref(),
        Some("1000000")
    );
    assert_eq!(
        serde_json::to_value(&history.estimate).unwrap(),
        serde_json::to_value(recovered.overall).unwrap()
    );
    // Recovery followed by a decrease closes a segment with only one endpoint.
    window(&mut store, "2026-01-01T00:05:00Z", "1", 1);
    window(&mut store, "2026-01-01T00:05:00Z", "2", 1);
    window(&mut store, "2026-01-01T00:06:00Z", "1", 1);
    window(&mut store, "2026-01-01T00:07:00Z", "0", 2);
    let insufficient = query(&mut store, "2026-01-01T00:07:00Z");
    let cycle = &insufficient.history[0];
    assert!(cycle.tokens.is_none());
    assert_eq!(
        cycle.estimate.unavailable_reason,
        Some(Unavailable::InsufficientObservations)
    );
    let unavailable_models = models(&mut store, &cycle.cycle.key, None, 10).unwrap();
    assert!(unavailable_models.items.is_empty());
    assert_eq!(
        unavailable_models.estimate.unavailable_reason,
        Some(Unavailable::InsufficientObservations)
    );
    window(&mut store, "2026-01-01T00:08:00Z", "0.5", 2);
    window(&mut store, "2026-01-01T00:09:00Z", "0", 3);
    let small = query(&mut store, "2026-01-01T00:09:00Z");
    assert_eq!(
        small.history[0].estimate.unavailable_reason,
        Some(Unavailable::BelowOnePercentagePoint)
    );
    assert!(
        small.history[0]
            .estimate
            .estimated_cost
            .as_ref()
            .unwrap()
            .complete
    );
    assert_eq!(
        small.history[0]
            .estimate
            .estimated_cost
            .as_ref()
            .unwrap()
            .known_subtotal
            .as_deref(),
        Some("0")
    );
    assert!(small.history[0]
        .tokens
        .as_ref()
        .unwrap()
        .total_tokens
        .known_tokens
        .is_none());
    assert!(
        !small.history[0]
            .tokens
            .as_ref()
            .unwrap()
            .total_tokens
            .complete
    );
}

#[test]
fn weekly_history_real_metadata_fixtures_unknown_models_and_exclusive_pages() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("real.sqlite")).unwrap();
    for (name, fixture) in [("active", super::ACTIVE), ("conflict", super::CONFLICT)] {
        let path = temp.path().join(format!("{name}.jsonl"));
        std::fs::write(&path, fixture).unwrap();
        crate::source::ingest(&mut store, &path).unwrap();
    }
    // Research excerpts preserve real token values; quota boundaries are synthetic.
    weekly(&mut store, "2026-01-01T11:30:00Z", "10");
    weekly(&mut store, "2026-01-01T11:40:00Z", "12");
    usage(&mut store, "priced-model", "2026-01-01T11:38:00Z", "priced");
    price(&mut store);
    window(&mut store, "2026-01-01T11:50:00Z", "0", 1);
    let result = query(&mut store, "2026-01-01T12:00:00Z");
    let cycle = &result.history[0];
    assert_eq!(
        cycle
            .tokens
            .as_ref()
            .unwrap()
            .total_tokens
            .known_tokens
            .as_deref(),
        Some("1079747")
    ); // 26587 + 53160 + 1000000.
    assert_eq!(
        cycle.estimate.unavailable_reason,
        Some(Unavailable::UnpricedUsage)
    );
    assert_eq!(
        cycle
            .estimate
            .estimated_cost
            .as_ref()
            .unwrap()
            .known_subtotal
            .as_deref(),
        Some("1000000000000")
    );
    let key = &cycle.cycle.key;
    let first = models(&mut store, key, None, 1).unwrap();
    assert_eq!(first.items[0].id, "model:gpt-5.6-terra");
    assert_eq!(
        first.items[0]
            .tokens
            .reasoning_tokens
            .known_tokens
            .as_deref(),
        Some("398")
    );
    assert!(!first.items[0].estimated_cost.complete);
    let second = models(&mut store, key, first.next_cursor, 1).unwrap();
    assert_eq!(second.items[0].id, "model:priced");
    assert!(second.items[0].estimated_cost.complete);
    let third = models(&mut store, key, second.next_cursor, 1).unwrap();
    assert_eq!(third.items[0].id, "unknown:");
    assert!(third.items[0].model.is_none());
    assert_eq!(
        third.items[0].tokens.total_tokens.known_tokens.as_deref(),
        Some("53160")
    );
    assert!(third.items[0].estimated_cost.known_subtotal.is_none());
    assert!(third.next_cursor.is_none());
    assert!(models(&mut store, key, Some("unknown:".into()), 1)
        .unwrap()
        .items
        .is_empty());
    assert!(models(&mut store, &result.current_cycle.unwrap().key, None, 1).is_none());
    assert!(models(&mut store, &time("2025-01-01T00:00:00Z").key(), None, 1).is_none());
    for (key, size) in [("bad".to_string(), 1), (key.clone(), 0), (key.clone(), 51)] {
        assert_eq!(
            store
                .weekly_models_at(
                    weekly::ModelsQuery {
                        cycle_key: key,
                        page: crate::aggregates::PageRequest {
                            after: None,
                            limit: size
                        },
                    },
                    time("2026-01-02T00:00:00Z")
                )
                .unwrap_err(),
            weekly::ReadError::InvalidQuery
        );
    }
}
