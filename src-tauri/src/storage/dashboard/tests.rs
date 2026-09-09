use super::*;
use crate::{
    adapter,
    dashboard::{BoundaryKind, Query, Range},
    pricing::{CacheWritePolicy, PriceInput, ReasoningPolicy},
    weekly::Unavailable,
};
use serde_json::json;

fn time(value: &str) -> Time {
    let (seconds, nanos) = adapter::observation_time(value).unwrap();
    Time { seconds, nanos }
}
fn record(store: &mut Store, path: &str, value: serde_json::Value) {
    let (offset, ordinal) = store.checkpoint(path).unwrap();
    let encoded = value.to_string();
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
fn limit(store: &mut Store, timestamp: &str, used: &str) {
    let used: serde_json::Number = serde_json::from_str(used).unwrap();
    record(
        store,
        "limits",
        json!({"type":"event_msg","timestamp":timestamp,"payload":{"type":"token_count","rate_limits":{"limit_id":"codex","secondary":{"used_percent":used,"window_minutes":10080}}}}),
    );
}
fn usage(store: &mut Store, thread: &str, timestamp: &str, model: &str) {
    record(
        store,
        thread,
        json!({"type":"session_meta","payload":{"id":thread}}),
    );
    record(
        store,
        thread,
        json!({"type":"turn_context","payload":{"turn_id":"turn","model":model}}),
    );
    let tokens = json!({"input_tokens":1000000,"cached_input_tokens":10,"cache_write_input_tokens":0,"output_tokens":0,"reasoning_output_tokens":0,"total_tokens":1000000});
    record(
        store,
        thread,
        json!({"type":"token_usage_record","timestamp":timestamp,"payload":{"thread_id":thread,"turn_id":"turn","response_id":"response","usage":tokens,"thread_token_usage":tokens}}),
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
fn quota_hypotheses_preserve_versions_model_mix_backfill_and_unknown_prices() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("hypotheses.sqlite");
    let mut store = Store::open(&path).unwrap();
    limit(&mut store, "2026-01-01T00:00:00Z", "10");
    usage(&mut store, "first", "2026-01-01T00:01:00Z", "priced");
    local_usage(
        &mut store,
        "second",
        Some("2026-01-01T00:02:00Z"),
        Some("other"),
        2_000_000,
        None,
        None,
    );
    limit(&mut store, "2026-01-01T00:03:00Z", "12");
    assert!(read(&mut store, "2026-01-01T00:03:00Z", Range::All)
        .quota_analysis
        .intervals[0]
        .hypotheses[1]
        .estimated_usd
        .is_none());
    price(&mut store);
    assert!(
        read(&mut store, "2026-01-01T00:03:00Z", Range::All)
            .quota_analysis
            .intervals[0]
            .hypotheses[1]
            .estimated_usd
            .is_none(),
        "one priced model cannot make the mixed interval complete"
    );
    let configuration = PriceInput {
        input: "3".into(),
        cached_input: "3".into(),
        cache_write: "3".into(),
        output: "3".into(),
        reasoning: None,
        reasoning_policy: ReasoningPolicy::Included,
        cache_write_policy: CacheWritePolicy::Additional,
    };
    store
        .save_model_price_at(
            "other",
            configuration.clone(),
            true,
            (time("2026-01-01T00:03:30Z").seconds, 0),
        )
        .unwrap();
    while store.pricing_work_pending().unwrap() {
        store.process_pricing_work().unwrap();
    }
    // 1M tokens at $1 + 2M at $3 = $7, not $3 (first rate) or $6 (mean rate).
    let before = read(&mut store, "2026-01-01T00:03:30Z", Range::All);
    assert_eq!(
        before.quota_analysis.intervals[0].hypotheses[1]
            .estimated_usd
            .as_deref(),
        Some("7000000000000")
    );
    usage(
        &mut store,
        "old-version-in-second",
        "2026-01-01T00:03:45Z",
        "priced",
    );
    store
        .save_model_price_at(
            "priced",
            configuration,
            false,
            (time("2026-01-01T00:04:00Z").seconds, 0),
        )
        .unwrap();
    usage(&mut store, "third", "2026-01-01T00:05:00Z", "priced");
    limit(&mut store, "2026-01-01T00:06:00Z", "14");
    while store.pricing_work_pending().unwrap() {
        store.process_pricing_work().unwrap();
    }
    drop(store);
    let mut store = Store::open(&path).unwrap();
    let after = read(&mut store, "2026-01-01T00:06:00Z", Range::All);
    assert_eq!(
        after.quota_analysis.intervals[0].hypotheses[1]
            .estimated_usd
            .as_deref(),
        Some("7000000000000")
    );
    assert_eq!(
        after.quota_analysis.intervals[1].hypotheses[1]
            .estimated_usd
            .as_deref(),
        Some("4000000000000")
    );
    // A later reset and a tied ambiguous observation never bridge intervals.
    limit(&mut store, "2026-01-01T00:07:00Z", "1");
    usage(&mut store, "fourth", "2026-01-01T00:08:00Z", "unknown");
    limit(&mut store, "2026-01-01T00:09:00Z", "3");
    limit(&mut store, "2026-01-01T00:10:00Z", "4");
    limit(&mut store, "2026-01-01T00:10:00Z", "5");
    limit(&mut store, "2026-01-01T00:11:00Z", "6");
    let after = read(&mut store, "2026-01-01T00:11:00Z", Range::All);
    let intervals = &after.quota_analysis.intervals;
    assert_eq!(intervals.len(), 3);
    assert_eq!(intervals[2].start, time("2026-01-01T00:07:00Z"));
    assert_eq!(intervals[2].end, time("2026-01-01T00:09:00Z"));
    assert!(intervals[2]
        .hypotheses
        .iter()
        .all(|row| row.tokens.is_some() && row.estimated_usd.is_none()));
}
fn read(store: &mut Store, now: &str, range: Range) -> dto::Response {
    store
        .dashboard_at(
            Query {
                range,
                point_budget: None,
                breakdown_metric: Default::default(),
            },
            time(now),
        )
        .unwrap()
}

fn local_usage(
    store: &mut Store,
    id: &str,
    timestamp: Option<&str>,
    model: Option<&str>,
    total: i64,
    project: Option<&str>,
    parent: Option<&str>,
) {
    record(
        store,
        id,
        json!({"type":"session_meta","payload":{"id":id,"cwd":project,"parent_thread_id":parent}}),
    );
    if let Some(model) = model {
        record(
            store,
            id,
            json!({"type":"turn_context","payload":{"turn_id":id,"model":model}}),
        );
    }
    let tokens = json!({"input_tokens":total,"cached_input_tokens":0,"cache_write_input_tokens":0,"output_tokens":0,"reasoning_output_tokens":0,"total_tokens":total});
    record(
        store,
        id,
        json!({"type":"token_usage_record","timestamp":timestamp,"payload":{"thread_id":id,"turn_id":id,"response_id":id,"usage":tokens,"thread_token_usage":tokens}}),
    );
}

#[test]
fn dashboard_local_usage_without_quota_preserves_all_bounds_and_unknown_cost() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("local.sqlite")).unwrap();
    let large = 9_007_199_254_740_993;
    local_usage(
        &mut store,
        "parent",
        Some("2026-01-01T00:00:00Z"),
        Some("priced"),
        large,
        Some("C:/work"),
        None,
    );
    local_usage(
        &mut store,
        "at-start",
        Some("2026-01-02T00:00:00Z"),
        None,
        19,
        None,
        None,
    );
    local_usage(
        &mut store,
        "child",
        Some("2026-01-02T00:00:00.000000001Z"),
        None,
        7,
        Some("C:/work"),
        Some("parent"),
    );
    local_usage(
        &mut store,
        "end",
        Some("2026-01-03T00:00:00Z"),
        None,
        11,
        None,
        None,
    );
    local_usage(
        &mut store,
        "future",
        Some("2026-01-04T00:00:00Z"),
        None,
        13,
        None,
        None,
    );
    local_usage(
        &mut store,
        "untimed",
        Some("2026-01-02T00:00:00Z"),
        None,
        17,
        None,
        None,
    );
    // Historical accepted rows can lack parsed observation time.
    store
        .connection
        .execute(
            "UPDATE observations SET time_seconds=NULL,time_nanos=NULL WHERE thread_id='untimed'",
            [],
        )
        .unwrap();
    price(&mut store);

    let all = read(&mut store, "2026-01-03T00:00:00Z", Range::All);
    assert!(all.chart.points.is_empty());
    assert!(all.weekly.current_cycle.is_none());
    assert_eq!(
        all.local_usage.start,
        time("2025-12-31T23:59:59.999999999Z")
    );
    assert_eq!(
        all.local_usage
            .summary
            .tokens
            .total_tokens
            .known_tokens
            .as_deref(),
        Some("9007199254741030")
    );
    assert_eq!(all.local_usage.summary.observed_sessions, 4);
    assert_eq!(all.local_usage.untimed_observations, 1);
    assert_eq!(
        all.local_usage
            .summary
            .estimated_cost
            .known_subtotal
            .as_deref(),
        Some("9007199254740993000000")
    );
    assert!(!all.local_usage.summary.estimated_cost.complete);
    assert_eq!(all.local_usage.points.len(), 3);
    assert_eq!(
        all.local_usage
            .points
            .iter()
            .map(|point| point
                .summary
                .tokens
                .total_tokens
                .known_tokens
                .as_deref()
                .unwrap()
                .parse::<i64>()
                .unwrap())
            .sum::<i64>(),
        large + 7 + 11 + 19
    );
    assert!(all
        .local_usage
        .points
        .iter()
        .all(|point| point.index < all.local_usage.bin_count));
    let json = serde_json::to_value(&all).unwrap();
    assert_eq!(
        json["localUsage"]["points"][0]["tokens"]["totalTokens"]["knownTokens"],
        large.to_string()
    );
    assert_eq!(json["breakdowns"]["metric"], "tokens");

    let day = read(&mut store, "2026-01-03T00:00:00Z", Range::Last24Hours);
    assert_eq!(
        day.local_usage
            .summary
            .tokens
            .total_tokens
            .known_tokens
            .as_deref(),
        Some("18")
    );
    assert_eq!(day.local_usage.summary.observed_sessions, 2);
    assert!(day
        .local_usage
        .summary
        .estimated_cost
        .known_subtotal
        .is_none());
    assert!(!day.local_usage.summary.estimated_cost.complete);
    assert_eq!(day.local_usage.points.len(), 2);
    let current = read(&mut store, "2026-01-03T00:00:00Z", Range::CurrentCycle);
    assert_eq!(
        current.local_usage.summary.tokens.total_tokens.known_tokens,
        all.local_usage.summary.tokens.total_tokens.known_tokens
    );
}

#[test]
fn dashboard_breakdowns_rank_full_range_keep_unknown_and_conserve_remainder() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("ranked.sqlite")).unwrap();
    for (index, model) in ["a", "b", "c", "d", "e", "f", "g"].into_iter().enumerate() {
        let total = (index as i64 + 1) * 10;
        local_usage(
            &mut store,
            model,
            Some("2026-01-02T12:00:00Z"),
            Some(model),
            total,
            Some(&format!("C:/ranking/{model}")),
            None,
        );
        let rate = (7 - index).to_string();
        store
            .save_model_price_at(
                model,
                PriceInput {
                    input: rate.clone(),
                    cached_input: rate.clone(),
                    cache_write: rate.clone(),
                    output: rate,
                    reasoning: None,
                    reasoning_policy: ReasoningPolicy::Included,
                    cache_write_policy: CacheWritePolicy::Unknown,
                },
                true,
                (0, 0),
            )
            .unwrap();
    }
    while store.pricing_work_pending().unwrap() {
        store.process_pricing_work().unwrap();
    }
    local_usage(
        &mut store,
        "unknown",
        Some("2026-01-02T12:00:00Z"),
        None,
        3,
        None,
        None,
    );
    local_usage(
        &mut store,
        "outside",
        Some("2026-01-01T00:00:00Z"),
        Some("outside"),
        1_000_000,
        Some("C:/outside"),
        None,
    );
    store.connection.execute("UPDATE sessions SET repository_state='confirmed',repository_common_directory='C:/ranking/g/.git' WHERE thread_id='g'", []).unwrap();
    let tokens = read(&mut store, "2026-01-03T00:00:00Z", Range::Last24Hours);
    assert_eq!(
        tokens
            .breakdowns
            .models
            .iter()
            .map(|row| row.key.as_str())
            .collect::<Vec<_>>(),
        ["model:g", "model:f", "model:e", "model:d", "model:c", "unknown:", "other:"]
    );
    assert_eq!(
        tokens
            .breakdowns
            .projects
            .iter()
            .map(|row| row.label.as_str())
            .collect::<Vec<_>>(),
        [
            "g",
            "f",
            "e",
            "d",
            "c",
            "Unattributed project",
            "Other projects"
        ]
    );
    for rows in [&tokens.breakdowns.models, &tokens.breakdowns.projects] {
        assert_eq!(
            rows.iter()
                .map(|row| row
                    .tokens
                    .known_tokens
                    .as_deref()
                    .unwrap()
                    .parse::<i64>()
                    .unwrap())
                .sum::<i64>(),
            283
        );
        assert_eq!(
            rows.last().unwrap().tokens.known_tokens.as_deref(),
            Some("30")
        );
        assert!(rows
            .iter()
            .find(|row| row.kind == "unknown")
            .unwrap()
            .estimated_cost
            .known_subtotal
            .is_none());
    }
    let costs = store
        .dashboard_at(
            Query {
                range: Range::Last24Hours,
                point_budget: Some(8),
                breakdown_metric: dto::BreakdownMetric::Cost,
            },
            time("2026-01-03T00:00:00Z"),
        )
        .unwrap();
    assert_eq!(
        costs
            .breakdowns
            .models
            .iter()
            .map(|row| row.key.as_str())
            .collect::<Vec<_>>(),
        ["model:d", "model:c", "model:e", "model:b", "model:f", "unknown:", "other:"]
    );
    assert_eq!(
        costs
            .breakdowns
            .models
            .last()
            .unwrap()
            .estimated_cost
            .known_subtotal
            .as_deref(),
        Some("140000000")
    );
    assert_eq!(
        costs
            .breakdowns
            .models
            .iter()
            .filter_map(|row| row.estimated_cost.known_subtotal.as_deref())
            .map(|amount| amount.parse::<i128>().unwrap())
            .sum::<i128>(),
        840_000_000
    );
    assert_eq!(costs.local_usage.bin_count, 1);
    assert_eq!(costs.local_usage.points.len(), 1);
    assert!(!costs.local_usage.summary.estimated_cost.complete);
}

#[test]
fn dashboard_exact_values_stale_unmatched_and_range_preserves_baseline() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("usage.sqlite")).unwrap();
    limit(&mut store, "2026-01-01T00:00:00Z", "40");
    limit(
        &mut store,
        "2026-01-02T00:00:00.000000001Z",
        "40.999999999999999999999999999999",
    );
    limit(&mut store, "2026-01-02T00:10:00.000000001Z", "42");
    usage(&mut store, "baseline", "2026-01-01T00:00:00Z", "unpriced");
    usage(&mut store, "inside", "2026-01-01T00:04:00Z", "priced");
    usage(
        &mut store,
        "at-end",
        "2026-01-02T00:10:00.000000001Z",
        "priced",
    );
    usage(&mut store, "newer", "2026-01-02T00:11:00Z", "unpriced");
    price(&mut store);
    let now = "2026-01-02T00:15:00.000000001Z";
    let all = read(&mut store, now, Range::All);
    let clipped = read(&mut store, now, Range::Last24Hours);
    assert_eq!(all.evaluated_at, all.weekly.evaluated_at);
    assert_eq!(all.evaluated_at, all.chart.end);
    assert_eq!(all.chart.points.len(), 3);
    assert_eq!(clipped.chart.points.len(), 2);
    assert_eq!(clipped.chart.points[0].time.nanos, 1);
    assert_eq!(
        clipped.chart.points[0].weekly_used_percent.as_deref(),
        Some("40.999999999999999999999999999999")
    );
    assert_eq!(
        clipped.chart.points[0].unavailable_reason,
        Some(Unavailable::BelowOnePercentagePoint)
    );
    let last = clipped.chart.points.last().unwrap();
    assert_eq!(
        last.cumulative_estimated_cost
            .as_ref()
            .unwrap()
            .known_subtotal
            .as_deref(),
        Some("2000000000000")
    );
    assert_eq!(
        last.effective_usd_per_percent.as_deref(),
        Some("1.000000000000")
    );
    assert_eq!(
        clipped.weekly.overall.start,
        Some(time("2026-01-01T00:00:00Z"))
    );
    assert_eq!(
        clipped.weekly.overall.effective_usd_per_percent,
        all.weekly.overall.effective_usd_per_percent
    );
    assert_eq!(clipped.weekly.observation_age_seconds, Some(300));
    assert!(!clipped.weekly.unmatched_cost.unwrap().complete);
    assert_eq!(
        clipped.global.tokens.total_tokens.known_tokens.as_deref(),
        Some("4000000")
    );
    assert_eq!(
        clipped
            .global
            .tokens
            .cached_input_tokens
            .known_tokens
            .as_deref(),
        Some("40")
    );
    assert!(clipped.token_scope.contains("All locally observed history"));
    let legacy = store
        .weekly_at(
            crate::weekly::Query {
                before: None,
                limit: 1,
            },
            time(now),
        )
        .unwrap();
    assert_eq!(
        serde_json::to_value(legacy).unwrap(),
        serde_json::to_value(all.weekly).unwrap()
    );
    let global = store.aggregates(crate::aggregates::Query::Global).unwrap();
    let crate::aggregates::Data::Global(global) = global.data else {
        panic!("global response");
    };
    assert_eq!(
        serde_json::to_value(global).unwrap(),
        serde_json::to_value(all.global).unwrap()
    );
}

#[test]
fn dashboard_reset_tie_and_incomplete_segments_restart_without_false_continuity() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("usage.sqlite")).unwrap();
    // Out-of-order ingestion and an equal-time conflict must use source chronology.
    for (minute, used) in [
        (4, "2"),
        (0, "40"),
        (1, "42"),
        (2, "1"),
        (3, "3"),
        (3, "50"),
        (5, "4"),
        (6, "5"),
        (7, "6"),
        (8, "7"),
    ] {
        limit(&mut store, &format!("2026-01-01T00:{minute:02}:00Z"), used);
    }
    usage(&mut store, "first", "2026-01-01T00:01:00Z", "priced");
    usage(&mut store, "missing", "2026-01-01T00:05:00Z", "unpriced");
    usage(&mut store, "recover", "2026-01-01T00:06:00Z", "priced");
    usage(&mut store, "after", "2026-01-01T00:07:00Z", "priced");
    price(&mut store);
    let response = read(&mut store, "2026-01-01T00:08:00Z", Range::All);
    let points = &response.chart.points;
    assert_eq!(points.len(), 9);
    assert_eq!(
        points[1].effective_usd_per_percent.as_deref(),
        Some("0.500000000000")
    );
    for index in [0, 2, 3, 4, 5, 6] {
        assert!(!points[index].connect_from_previous, "boundary at {index}");
    }
    assert_eq!(
        points[2]
            .cumulative_estimated_cost
            .as_ref()
            .unwrap()
            .known_subtotal
            .as_deref(),
        Some("0")
    );
    assert!(points[3].weekly_used_percent.is_none());
    assert_eq!(
        points[3].unavailable_reason,
        Some(Unavailable::AmbiguousObservation)
    );
    assert_eq!(
        points[5].unavailable_reason,
        Some(Unavailable::UnpricedUsage)
    );
    assert!(points[5]
        .cumulative_estimated_cost
        .as_ref()
        .unwrap()
        .known_subtotal
        .is_none());
    assert_eq!(
        points[6]
            .cumulative_estimated_cost
            .as_ref()
            .unwrap()
            .known_subtotal
            .as_deref(),
        Some("0")
    );
    assert_eq!(
        points[7].effective_usd_per_percent.as_deref(),
        Some("1.000000000000")
    );
    assert!(points[7].connect_from_previous);
    let kinds: Vec<_> = response
        .chart
        .boundaries
        .iter()
        .flat_map(|boundary| boundary.kinds.iter())
        .collect();
    for kind in [
        BoundaryKind::Reset,
        BoundaryKind::AmbiguousObservation,
        BoundaryKind::UnpricedUsage,
        BoundaryKind::PricedUsageResumed,
    ] {
        assert!(kinds.contains(&&kind));
    }
    // Card baseline still spans the whole comparable quota interval and is incomplete.
    assert_eq!(
        response.weekly.overall.unavailable_reason,
        Some(Unavailable::UnpricedUsage)
    );
    let current = read(&mut store, "2026-01-01T00:08:00Z", Range::CurrentCycle);
    assert_eq!(current.chart.start, time("2026-01-01T00:02:00Z"));
    assert_eq!(current.chart.points.len(), 7);
}

#[test]
fn dashboard_downsampling_retains_actual_extrema_and_caps_boundary_overload() {
    use crate::dashboard::{Downsample, Point};
    let start = Time {
        seconds: 0,
        nanos: 0,
    };
    let end = Time {
        seconds: 99,
        nanos: 0,
    };
    let mut sample = Downsample::new(Range::All, start, end, 1);
    for index in 0..100 {
        let used = if index == 25 {
            "99"
        } else if index == 26 {
            "0"
        } else {
            "50"
        };
        let amount = if index == 40 {
            "9000000000000"
        } else if index == 41 {
            "1"
        } else {
            "1000000000000"
        };
        let ratio = if index == 60 {
            "9.000000000001"
        } else if index == 61 {
            "0.000000000001"
        } else {
            "1.000000000000"
        };
        sample.push(
            Point {
                time: Time {
                    seconds: index,
                    nanos: 0,
                },
                segment_id: Some("segment".into()),
                weekly_used_percent: Some(used.into()),
                cumulative_estimated_cost: Some(Cost {
                    known_subtotal: Some(amount.into()),
                    complete: true,
                    accepted_observations: 1,
                }),
                effective_usd_per_percent: Some(ratio.into()),
                unavailable_reason: None,
                connect_from_previous: false,
            },
            (index % 2 == 0).then_some(BoundaryKind::Reset),
        );
    }
    let chart = sample.finish();
    assert_eq!(
        chart
            .points
            .iter()
            .map(|point| point.time.seconds)
            .collect::<Vec<_>>(),
        [0, 25, 26, 40, 41, 60, 61, 99]
    );
    assert_eq!(chart.source_observation_count, 100);
    assert_eq!(chart.returned_observation_count, 8);
    assert_eq!(chart.boundaries.len(), 1);
    assert_eq!(chart.boundaries[0].count, 50);
    assert!(chart.boundaries[0].overloaded);
    assert!(chart
        .points
        .iter()
        .all(|point| !point.connect_from_previous));
    for budget in [0, 7, 4097, u32::MAX] {
        assert_eq!(
            Query {
                range: Range::All,
                point_budget: Some(budget),
                breakdown_metric: Default::default(),
            }
            .validate(),
            Err(ReadError::InvalidQuery)
        );
    }
    assert_eq!(
        Query {
            range: Range::All,
            point_budget: None,
            breakdown_metric: Default::default(),
        }
        .validate(),
        Ok(512)
    );
    assert_eq!(
        Query {
            range: Range::All,
            point_budget: Some(15),
            breakdown_metric: Default::default(),
        }
        .validate(),
        Ok(1)
    );
}

#[test]
fn dashboard_empty_read_only_serialization_and_invalid_input() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("usage.sqlite");
    let mut store = Store::open(&path).unwrap();
    let response = read(&mut store, "2026-01-01T00:00:00Z", Range::All);
    assert!(response.chart.points.is_empty());
    assert!(response.weekly.current_cycle.is_none());
    assert!(response.global.tokens.total_tokens.known_tokens.is_none());
    assert!(response.weekly.overall.effective_usd_per_percent.is_none());
    let encoded = serde_json::to_value(response).unwrap();
    assert_eq!(encoded["chart"]["range"], "all");
    assert_eq!(encoded["chart"]["binCount"], 512);
    assert!(encoded["global"]["tokens"]["cacheWriteTokens"]["knownTokens"].is_null());
    assert!(serde_json::from_value::<Query>(json!({"range":"all","unexpected":1})).is_err());
    assert!(serde_json::from_value::<Query>(json!({"range":"unsupported"})).is_err());
    drop(store);
    assert!(Store::read_dashboard(
        &path,
        Query {
            range: Range::All,
            point_budget: Some(8),
            breakdown_metric: Default::default(),
        }
    )
    .is_ok());
    let missing = temp.path().join("missing.sqlite");
    assert_eq!(
        Store::read_dashboard(
            &missing,
            Query {
                range: Range::All,
                point_budget: None,
                breakdown_metric: Default::default(),
            }
        )
        .unwrap_err(),
        ReadError::Storage
    );
    assert!(!missing.exists());
}

#[test]
fn dashboard_range_boundaries_are_exact() {
    let now = Time {
        seconds: 10_000_000,
        nanos: 7,
    };
    for (range, days) in [
        (Range::Last24Hours, 1),
        (Range::Last7Days, 7),
        (Range::Last30Days, 30),
    ] {
        assert_eq!(
            Query {
                range,
                point_budget: None,
                breakdown_metric: Default::default(),
            }
            .start(now, None, None),
            Time {
                seconds: now.seconds - days * 86400,
                nanos: 7
            }
        );
    }
}

#[test]
fn dashboard_large_history_is_bounded_and_reports_query_plan() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("history.sqlite")).unwrap();
    // Normalized test metadata only; 6,000 canonical observations and 2,999
    // chronological resets deliberately exceed the boundary-summary budget.
    store.connection.execute_batch("WITH RECURSIVE series(n) AS (VALUES(0) UNION ALL SELECT n+1 FROM series WHERE n<5999) INSERT INTO limit_samples(bucket,position,window_minutes,timestamp,used_percent,normalized,source_path,source_offset,adapter) SELECT 'codex','secondary',10080,strftime('%Y-%m-%dT%H:%M:%SZ',1767225600+n,'unixepoch'),CAST((n%2)*50 AS TEXT),CAST(n AS TEXT),'fixture',n,'fixture' FROM series").unwrap();
    let started = std::time::Instant::now();
    let response = read(&mut store, "2026-01-01T01:39:59Z", Range::All);
    let elapsed = started.elapsed();
    assert_eq!(response.chart.source_observation_count, 6000);
    assert!(response.chart.points.len() <= 4096);
    assert!(response.chart.boundaries.len() <= 512);
    assert_eq!(
        response
            .chart
            .boundaries
            .iter()
            .map(|boundary| boundary.count)
            .sum::<u64>(),
        3000
    );
    assert!(response
        .chart
        .boundaries
        .iter()
        .all(|boundary| boundary.overloaded));
    assert!(response
        .chart
        .points
        .iter()
        .all(|point| !point.connect_from_previous));
    assert_eq!(
        response.chart.points.first().unwrap().time,
        time("2026-01-01T00:00:00Z")
    );
    assert_eq!(
        response.chart.points.last().unwrap().time,
        time("2026-01-01T01:39:59Z")
    );
    let plans: Vec<String> = store
        .connection
        .prepare(&format!("EXPLAIN QUERY PLAN {COST_GROUPS}"))
        .unwrap()
        .query_map(params![1767231599, 0], |row| row.get(3))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    eprintln!("dashboard synthetic history: 6000 limits, 0 costs, {} returned points, {} boundary bins, elapsed {:?}; cost query plan: {:?}", response.chart.points.len(), response.chart.boundaries.len(), elapsed, plans);
}

#[test]
fn dashboard_month_of_mixed_cost_history_reports_bounded_payload() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("mixed-history.sqlite")).unwrap();
    // Read-side fixture: 300 sessions, one accepted 1,000-token observation per
    // minute for 30 days, and quota samples every five minutes. Daily one-hour
    // unpriced stretches exercise incomplete/recovered segments; quota decreases
    // every seven days exercise cycle resets. No source content is involved.
    store.connection.execute_batch(
        "BEGIN;
        INSERT INTO detected_models(model) VALUES('priced'),('unpriced');
        WITH RECURSIVE series(n) AS (VALUES(0) UNION ALL SELECT n+1 FROM series WHERE n<299)
        INSERT INTO sessions(thread_id) SELECT 'fixture:' || n FROM series;
        WITH RECURSIVE series(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM series WHERE n<43200),
        metadata AS (SELECT n,'fixture:' || ((n-1)%300) AS thread,
            json_object('input_tokens',1000,'cached_input_tokens',100,'cache_write_input_tokens',0,
                'output_tokens',0,'reasoning_output_tokens',0,'total_tokens',1000) AS tokens,
            json_object('input_tokens',1000*((n-1)/300+1),'cached_input_tokens',100*((n-1)/300+1),
                'cache_write_input_tokens',0,'output_tokens',0,'reasoning_output_tokens',0,
                'total_tokens',1000*((n-1)/300+1)) AS cumulative FROM series)
        INSERT INTO observations(thread_id,endpoint,timestamp,normalized,adapter,source_path,
            source_offset,source_ordinal,model,accepted,total,state,time_seconds,time_nanos)
        SELECT thread,CAST(n AS TEXT),strftime('%Y-%m-%dT%H:%M:%SZ',1767225600+n*60,'unixepoch'),
            json_object('thread_id',thread,'usage',json(tokens),'thread_token_usage',json(cumulative)),
            'fixture','fixture',n,n,CASE WHEN n%1440 BETWEEN 600 AND 659 THEN 'unpriced' ELSE 'priced' END,
            1,1000,'accepted',1767225600+n*60,0 FROM metadata;
        WITH RECURSIVE series(n) AS (VALUES(0) UNION ALL SELECT n+1 FROM series WHERE n<8640)
        INSERT INTO limit_samples(bucket,position,window_minutes,timestamp,used_percent,normalized,
            source_path,source_offset,adapter)
        SELECT 'codex','secondary',10080,strftime('%Y-%m-%dT%H:%M:%SZ',1767225600+n*300,'unixepoch'),
            CAST((n%2016)/24 AS TEXT),CAST(n AS TEXT),'fixture',n,'fixture' FROM series;
        COMMIT;",
    ).unwrap();
    // Populate immutable valuations through the existing pricing owner, outside
    // the projection timing. $1 per million tokens values each priced row at $0.001.
    price(&mut store);
    let priced_rows: i64 = store
        .connection
        .query_row("SELECT COUNT(*) FROM observation_valuations", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(priced_rows, 43200 - 30 * 60);

    let started = std::time::Instant::now();
    let response = read(&mut store, "2026-01-31T00:00:00Z", Range::All);
    let elapsed = started.elapsed();
    let payload_bytes = serde_json::to_vec(&response).unwrap().len();
    assert_eq!(response.chart.source_observation_count, 8641);
    assert!(response.chart.points.len() <= 4096);
    assert!(response.chart.boundaries.len() <= 512);
    assert_eq!(
        response.global.tokens.total_tokens.known_tokens.as_deref(),
        Some("43200000")
    );
    assert_eq!(
        response.global.estimated_cost.known_subtotal.as_deref(),
        Some("41400000000000")
    );
    assert!(!response.global.estimated_cost.complete);
    assert_eq!(
        response
            .chart
            .boundaries
            .iter()
            .map(|boundary| boundary.count)
            .sum::<u64>(),
        65
    );
    assert!(response
        .chart
        .boundaries
        .iter()
        .any(|boundary| boundary.kinds.contains(&BoundaryKind::Reset)));
    assert!(response
        .chart
        .boundaries
        .iter()
        .any(|boundary| boundary.kinds.contains(&BoundaryKind::UnpricedUsage)));
    assert!(response
        .chart
        .boundaries
        .iter()
        .any(|boundary| boundary.kinds.contains(&BoundaryKind::PricedUsageResumed)));
    assert!(response
        .chart
        .points
        .iter()
        .any(|point| point.unavailable_reason == Some(Unavailable::UnpricedUsage)));
    assert!(response
        .chart
        .points
        .iter()
        .any(|point| point.effective_usd_per_percent.is_some()));
    // The final recovered segment begins at day 30 11:05 (after the daily
    // incomplete interval), leaving 775 priced minutes. The quota is 17 then
    // (421 / 24) and 24 at the endpoint (576 / 24): 7 consumed points.
    let last = response.chart.points.last().unwrap();
    assert_eq!(
        last.cumulative_estimated_cost
            .as_ref()
            .unwrap()
            .known_subtotal
            .as_deref(),
        Some("775000000000")
    );
    assert_eq!(
        last.effective_usd_per_percent.as_deref(),
        Some("0.110714285714")
    );
    eprintln!("dashboard mixed history: 30 days, 300 sessions, 8641 limits, 43200 accepted observations, {priced_rows} immutable valuations; {} returned points, {} boundary bins, {payload_bytes} serialized bytes, projection elapsed {elapsed:?}", response.chart.points.len(), response.chart.boundaries.len());
}

#[test]
fn quota_hypotheses_apply_the_bounded_reach_back_before_valuation_persists() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("reach-back.sqlite");
    let mut store = Store::open(&path).unwrap();
    limit(&mut store, "2026-01-01T00:00:00Z", "10");
    usage(&mut store, "recent", "2026-01-01T00:01:00Z", "priced");
    limit(&mut store, "2026-01-01T00:03:00Z", "12");
    usage(&mut store, "other", "2026-01-01T00:04:00Z", "late");
    limit(&mut store, "2026-01-01T00:06:00Z", "14");
    let configuration = PriceInput {
        input: "1".into(),
        cached_input: "1".into(),
        cache_write: "1".into(),
        output: "1".into(),
        reasoning: None,
        reasoning_policy: ReasoningPolicy::Included,
        cache_write_policy: CacheWritePolicy::Additional,
    };
    // Two days later is inside the reach-back; eight days later is beyond it.
    for (model, effective) in [
        ("priced", "2026-01-03T00:00:00Z"),
        ("late", "2026-01-09T00:00:00Z"),
    ] {
        store
            .save_model_price_at(
                model,
                configuration.clone(),
                false,
                (time(effective).seconds, 0),
            )
            .unwrap();
    }
    assert!(store.pricing_work_pending().unwrap());
    let expected = |store: &mut Store| {
        let intervals = read(store, "2026-01-09T00:00:00Z", Range::All)
            .quota_analysis
            .intervals;
        assert_eq!(intervals.len(), 2);
        assert_eq!(
            intervals[0].hypotheses[1].estimated_usd.as_deref(),
            Some("1000000000000")
        );
        assert_eq!(
            (
                intervals[1].hypotheses[1].estimated_usd.as_deref(),
                intervals[1].hypotheses[1].price_reason
            ),
            (None, Some("Unpriced usage: no applicable model price"))
        );
    };
    expected(&mut store);
    while store.pricing_work_pending().unwrap() {
        store.process_pricing_work().unwrap();
    }
    assert!(store.observation_valuation(1).unwrap().is_some());
    assert!(store.observation_valuation(2).unwrap().is_none());
    expected(&mut store);
}

#[test]
fn quota_category_costs_use_each_observations_model_price_or_stay_unavailable() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("categories.sqlite")).unwrap();
    limit(&mut store, "2026-01-01T00:00:00Z", "10");
    local_usage(
        &mut store,
        "first",
        Some("2026-01-01T00:01:00Z"),
        Some("priced"),
        1_000_000,
        None,
        None,
    );
    local_usage(
        &mut store,
        "second",
        Some("2026-01-01T00:02:00Z"),
        Some("other"),
        2_000_000,
        None,
        None,
    );
    limit(&mut store, "2026-01-01T00:03:00Z", "12");
    price(&mut store);
    let partial = read(&mut store, "2026-01-01T00:03:00Z", Range::All);
    let categories = &partial.quota_analysis.intervals[0].categories;
    assert!(categories.input.is_none() && categories.output.is_none());
    assert_eq!(
        categories.reason,
        Some("Unpriced usage: no applicable model price")
    );
    store
        .save_model_price_at(
            "other",
            PriceInput {
                input: "3".into(),
                cached_input: "3".into(),
                cache_write: "3".into(),
                output: "3".into(),
                reasoning: None,
                reasoning_policy: ReasoningPolicy::Included,
                cache_write_policy: CacheWritePolicy::Additional,
            },
            true,
            (0, 1),
        )
        .unwrap();
    while store.pricing_work_pending().unwrap() {
        store.process_pricing_work().unwrap();
    }
    let complete = read(&mut store, "2026-01-01T00:03:00Z", Range::All);
    let categories = &complete.quota_analysis.intervals[0].categories;
    // 1M input at $1 plus 2M input at $3: each observation at its own price.
    assert_eq!(
        [
            categories.input.as_deref(),
            categories.cached_input.as_deref(),
            categories.cache_writes.as_deref(),
            categories.output.as_deref(),
            categories.reason,
        ],
        [Some("7000000000000"), Some("0"), Some("0"), Some("0"), None]
    );
}

fn categorized_usage(
    store: &mut Store,
    id: &str,
    timestamp: &str,
    model: Option<&str>,
    categories: [i64; 5],
) {
    record(
        store,
        id,
        json!({"type":"session_meta","payload":{"id":id,"cwd":format!("C:/costs/{id}")}}),
    );
    if let Some(model) = model {
        record(
            store,
            id,
            json!({"type":"turn_context","payload":{"turn_id":id,"model":model}}),
        );
    }
    let [input, cached, writes, output, reasoning] = categories;
    let tokens = json!({"input_tokens":input,"cached_input_tokens":cached,"cache_write_input_tokens":writes,"output_tokens":output,"reasoning_output_tokens":reasoning,"total_tokens":input+output});
    record(
        store,
        id,
        json!({"type":"token_usage_record","timestamp":timestamp,"payload":{"thread_id":id,"turn_id":id,"response_id":id,"usage":tokens,"thread_token_usage":tokens}}),
    );
}

#[allow(clippy::too_many_arguments)]
fn save_price(
    store: &mut Store,
    model: &str,
    input: &str,
    cached: &str,
    write: &str,
    output: &str,
    reasoning: Option<&str>,
    writes: CacheWritePolicy,
) {
    store
        .save_model_price_at(
            model,
            PriceInput {
                input: input.into(),
                cached_input: cached.into(),
                cache_write: write.into(),
                output: output.into(),
                reasoning: reasoning.map(str::to_owned),
                reasoning_policy: if reasoning.is_some() {
                    ReasoningPolicy::Separate
                } else {
                    ReasoningPolicy::Included
                },
                cache_write_policy: writes,
            },
            true,
            (0, 0),
        )
        .unwrap();
    while store.pricing_work_pending().unwrap() {
        store.process_pricing_work().unwrap();
    }
}

fn split(row: &dto::ModelCost) -> [Option<&str>; 4] {
    [
        row.categories.input.as_deref(),
        row.categories.cached_input.as_deref(),
        row.categories.cache_writes.as_deref(),
        row.categories.output.as_deref(),
    ]
}

#[test]
fn model_costs_split_each_models_subtotal_by_category_under_its_own_policies() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("model-costs.sqlite")).unwrap();
    categorized_usage(
        &mut store,
        "alpha",
        "2026-01-02T12:00:00Z",
        Some("alpha"),
        [1000, 400, 100, 200, 50],
    );
    categorized_usage(
        &mut store,
        "beta",
        "2026-01-02T12:00:00Z",
        Some("beta"),
        [2000, 500, 200, 100, 40],
    );
    categorized_usage(
        &mut store,
        "nameless",
        "2026-01-02T12:00:00Z",
        None,
        [7, 0, 0, 3, 0],
    );
    save_price(
        &mut store,
        "alpha",
        "2",
        "0.5",
        "3",
        "4",
        None,
        CacheWritePolicy::Additional,
    );
    save_price(
        &mut store,
        "beta",
        "1",
        "0.25",
        "2",
        "8",
        Some("16"),
        CacheWritePolicy::IncludedInputDisjoint,
    );

    let response = read(&mut store, "2026-01-03T00:00:00Z", Range::Last24Hours);
    let rows = &response.breakdowns.model_costs;
    assert_eq!(
        rows.iter().map(|row| row.key.as_str()).collect::<Vec<_>>(),
        ["model:beta", "model:alpha", "unknown:"],
        "priced models rank by estimated cost; unknown attribution stays last"
    );

    // beta: 1300 fresh input, 500 cached, 200 writes taken out of input, then
    // 60 output plus 40 separately priced reasoning tokens.
    let beta = &rows[0];
    assert_eq!(
        split(beta),
        [
            Some("1300000000"),
            Some("125000000"),
            Some("400000000"),
            Some("1120000000")
        ]
    );
    // alpha: cache writes are additional to input, reasoning is inside output.
    let alpha = &rows[1];
    assert_eq!(
        split(alpha),
        [
            Some("1200000000"),
            Some("200000000"),
            Some("300000000"),
            Some("800000000")
        ]
    );
    for row in [beta, alpha] {
        let total: i128 = split(row)
            .into_iter()
            .map(|amount| amount.unwrap().parse::<i128>().unwrap())
            .sum();
        assert_eq!(
            Some(total.to_string()),
            row.estimated_cost.known_subtotal,
            "the four amounts reconstruct the model's known cost subtotal"
        );
        assert!(row.estimated_cost.complete && row.categories.reason.is_none());
        assert_eq!(row.accepted_observations, 1);
        assert_eq!(row.observed_sessions, Some(1));
    }
    assert_eq!(
        [
            beta.tokens.input_tokens.known_tokens.as_deref(),
            beta.tokens.cached_input_tokens.known_tokens.as_deref(),
            beta.tokens.cache_write_tokens.known_tokens.as_deref(),
            beta.tokens.output_tokens.known_tokens.as_deref(),
            beta.tokens.reasoning_tokens.known_tokens.as_deref(),
        ],
        [
            Some("2000"),
            Some("500"),
            Some("200"),
            Some("100"),
            Some("40")
        ]
    );

    let unknown = &rows[2];
    assert_eq!(unknown.label, "Unknown model");
    assert!(unknown.estimated_cost.known_subtotal.is_none() && !unknown.estimated_cost.complete);
    assert_eq!(
        unknown.categories.reason,
        Some("Unpriced usage: no applicable model price")
    );
    assert_eq!(
        unknown.tokens.total_tokens.known_tokens.as_deref(),
        Some("10")
    );

    // Unknown-model usage leaves the split of the priced remainder intact.
    let totals = &response.breakdowns.category_totals;
    assert_eq!(
        [
            totals.input.as_deref(),
            totals.cached_input.as_deref(),
            totals.cache_writes.as_deref(),
            totals.output.as_deref(),
        ],
        [
            Some("2500000000"),
            Some("325000000"),
            Some("700000000"),
            Some("1920000000")
        ]
    );
    assert_eq!(totals.reason, None);
    assert_eq!(
        response.breakdowns.models[0].estimated_cost.known_subtotal,
        beta.estimated_cost.known_subtotal,
        "the cost table and the ranked breakdown agree on a model's subtotal"
    );
}

#[test]
fn model_costs_fold_the_remainder_and_keep_partly_priced_models_honest() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("model-fold.sqlite")).unwrap();
    for index in 0..10i64 {
        let model = format!("m{index:02}");
        categorized_usage(
            &mut store,
            &model,
            "2026-01-02T12:00:00Z",
            Some(&model),
            [(index + 1) * 100, 0, 0, 0, 0],
        );
        save_price(
            &mut store,
            &model,
            "1",
            "1",
            "1",
            "1",
            None,
            CacheWritePolicy::Unknown,
        );
    }
    // A second observation for the busiest model, with cache writes its price
    // version cannot interpret, so it is accepted but never valued.
    categorized_usage(
        &mut store,
        "m09-again",
        "2026-01-02T13:00:00Z",
        Some("m09"),
        [500, 0, 50, 0, 0],
    );
    while store.pricing_work_pending().unwrap() {
        store.process_pricing_work().unwrap();
    }

    let response = read(&mut store, "2026-01-03T00:00:00Z", Range::Last24Hours);
    let rows = &response.breakdowns.model_costs;
    assert_eq!(rows.len(), 9, "eight named models plus one folded remainder");
    assert_eq!(
        (rows[8].key.as_str(), rows[8].label.as_str()),
        ("other:", "Other models (2)")
    );
    assert_eq!(
        rows[8].tokens.total_tokens.known_tokens.as_deref(),
        Some("300"),
        "the folded row conserves the remaining tokens exactly"
    );
    assert_eq!(rows[8].accepted_observations, 2);
    assert_eq!(
        rows[8].categories.input.as_deref(),
        Some("300000000"),
        "folded amounts add up rather than being dropped"
    );

    let busiest = &rows[0];
    assert_eq!(busiest.key, "model:m09");
    assert_eq!(busiest.accepted_observations, 2);
    assert!(
        !busiest.estimated_cost.complete,
        "an observation still waiting for its valuation stays visibly incomplete"
    );
    assert_eq!(
        busiest.categories.input.as_deref(),
        busiest.estimated_cost.known_subtotal.as_deref(),
        "the split covers exactly the known subtotal, never the unvalued remainder"
    );
    assert_eq!(busiest.categories.input.as_deref(), Some("1000000000"));
    assert_eq!(
        busiest.tokens.total_tokens.known_tokens.as_deref(),
        Some("1500")
    );
}

fn turn_context(store: &mut Store, thread: &str, turn: &str, model: &str, effort: Option<&str>) {
    record(
        store,
        thread,
        json!({"type":"turn_context","payload":{"turn_id":turn,"model":model,"effort":effort}}),
    );
}

/// `usage` is the turn's own delta; `cumulative` is the thread total it reaches.
fn turn_usage(
    store: &mut Store,
    thread: &str,
    turn: Option<&str>,
    timestamp: &str,
    input: i64,
    cumulative: i64,
) {
    let counters = |value: i64| {
        json!({"input_tokens":value,"cached_input_tokens":0,"cache_write_input_tokens":0,"output_tokens":0,"reasoning_output_tokens":0,"total_tokens":value})
    };
    record(
        store,
        thread,
        json!({"type":"token_usage_record","timestamp":timestamp,"payload":{"thread_id":thread,"turn_id":turn,"response_id":format!("{thread}-{}-{timestamp}",turn.unwrap_or("anonymous")),"usage":counters(input),"thread_token_usage":counters(cumulative)}}),
    );
}

fn session(store: &mut Store, thread: &str) {
    record(
        store,
        thread,
        json!({"type":"session_meta","payload":{"id":thread}}),
    );
}

fn series_totals(series: &dto::TurnSeries) -> (u64, i64) {
    (
        series.points.iter().map(|point| point.turns).sum(),
        series
            .points
            .iter()
            .map(|point| {
                point
                    .tokens
                    .known_tokens
                    .as_deref()
                    .unwrap_or("0")
                    .parse::<i64>()
                    .unwrap()
            })
            .sum(),
    )
}

#[test]
fn turn_activity_counts_turns_per_model_and_reasoning_and_bins_them_once() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("turns.sqlite")).unwrap();

    session(&mut store, "s1");
    turn_context(&mut store, "s1", "t1", "alpha", Some("high"));
    turn_usage(&mut store, "s1", Some("t1"), "2026-01-02T12:00:00Z", 100, 100);
    turn_usage(&mut store, "s1", Some("t1"), "2026-01-02T12:01:00Z", 100, 200);
    turn_context(&mut store, "s1", "t2", "alpha", Some("high"));
    turn_usage(&mut store, "s1", Some("t2"), "2026-01-02T12:30:00Z", 100, 300);
    turn_context(&mut store, "s1", "t3", "alpha", Some("low"));
    turn_usage(&mut store, "s1", Some("t3"), "2026-01-02T13:00:00Z", 150, 450);

    session(&mut store, "s2");
    turn_context(&mut store, "s2", "t4", "beta", Some("medium"));
    turn_usage(&mut store, "s2", Some("t4"), "2026-01-02T14:05:00Z", 250, 250);
    turn_context(&mut store, "s2", "t5", "beta", Some("medium"));
    turn_usage(&mut store, "s2", Some("t5"), "2026-01-02T14:20:00Z", 250, 500);

    session(&mut store, "s3");
    turn_context(&mut store, "s3", "t6", "beta", None);
    turn_usage(&mut store, "s3", Some("t6"), "2026-01-02T15:00:00Z", 120, 120);

    // No turn context and no turn identity: neither the model nor the reasoning
    // can be attributed, and the observation stands as its own turn.
    session(&mut store, "s4");
    turn_usage(&mut store, "s4", None, "2026-01-02T16:00:00Z", 90, 90);

    let response = read(&mut store, "2026-01-03T00:00:00Z", Range::Last24Hours);
    let activity = &response.turn_activity;
    assert_eq!(activity.bin_count, 96);
    assert_eq!(activity.total_turns, 7);
    assert_eq!(activity.combinations, 5);
    assert_eq!(activity.turns_without_identity, 1);
    assert_eq!(
        activity
            .series
            .iter()
            .map(|series| (series.key.as_str(), series.turns))
            .collect::<Vec<_>>(),
        [
            ("model:beta|effort:medium", 2),
            ("model:alpha|effort:high", 2),
            ("model:alpha|effort:low", 1),
            ("model:beta|effort:", 1),
            ("unknown:|effort:", 1),
        ],
        "turns rank first, then the work those turns carried"
    );
    assert_eq!(
        activity
            .series
            .iter()
            .map(|series| series.label.as_str())
            .collect::<Vec<_>>(),
        [
            "beta · medium",
            "alpha · high",
            "alpha · low",
            "beta · reasoning unavailable",
            "Unknown model · reasoning unavailable"
        ]
    );
    assert_eq!(
        activity
            .series
            .iter()
            .map(|series| (series.model.as_deref(), series.effort.as_deref()))
            .collect::<Vec<_>>(),
        [
            (Some("beta"), Some("medium")),
            (Some("alpha"), Some("high")),
            (Some("alpha"), Some("low")),
            (Some("beta"), None),
            (None, None)
        ]
    );

    let busiest = &activity.series[1];
    assert_eq!(busiest.accepted_observations, 3);
    assert_eq!(busiest.observed_sessions, Some(1));
    assert_eq!(busiest.tokens.known_tokens.as_deref(), Some("300"));
    // A turn is charted where it began, so its second observation joins the
    // first bin instead of opening one of its own.
    assert_eq!(
        busiest
            .points
            .iter()
            .map(|point| (
                point.index,
                point.turns,
                point.tokens.known_tokens.as_deref()
            ))
            .collect::<Vec<_>>(),
        [(47, 1, Some("200")), (49, 1, Some("100"))]
    );
    for series in &activity.series {
        assert_eq!(
            series_totals(series),
            (
                series.turns,
                series.tokens.known_tokens.as_deref().unwrap().parse().unwrap()
            ),
            "every bin adds up to the series total for {}",
            series.key
        );
    }
    assert_eq!(
        activity
            .series
            .iter()
            .map(|series| series.turns)
            .sum::<u64>(),
        activity.total_turns
    );
    assert!(activity
        .series
        .iter()
        .all(|series| series.estimated_cost.known_subtotal.is_none()
            && !series.estimated_cost.complete));
}

#[test]
fn turn_activity_keeps_the_model_when_only_the_reasoning_context_conflicts() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("turn-conflict.sqlite")).unwrap();
    session(&mut store, "s1");
    turn_context(&mut store, "s1", "t1", "alpha", Some("high"));
    turn_context(&mut store, "s1", "t1", "alpha", Some("low"));
    turn_usage(&mut store, "s1", Some("t1"), "2026-01-02T12:00:00Z", 100, 100);
    // A model conflict on a different turn still removes only that attribute.
    session(&mut store, "s2");
    turn_context(&mut store, "s2", "t2", "alpha", Some("high"));
    turn_context(&mut store, "s2", "t2", "beta", Some("high"));
    turn_usage(&mut store, "s2", Some("t2"), "2026-01-02T12:00:00Z", 200, 200);

    let response = read(&mut store, "2026-01-03T00:00:00Z", Range::Last24Hours);
    assert_eq!(
        response
            .turn_activity
            .series
            .iter()
            .map(|series| (series.model.as_deref(), series.effort.as_deref(), series.turns))
            .collect::<Vec<_>>(),
        [(None, Some("high"), 1), (Some("alpha"), None, 1)],
        "a disagreement about one attribute never discards the other"
    );
}

#[test]
fn turn_activity_folds_the_remainder_into_one_series() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("turn-fold.sqlite")).unwrap();
    // Ten combinations, each with one more turn than the last.
    for index in 0..10i64 {
        let thread = format!("s{index:02}");
        session(&mut store, &thread);
        let mut cumulative = 0;
        for turn in 0..=index {
            let id = format!("t{index}-{turn}");
            turn_context(&mut store, &thread, &id, "alpha", Some(&format!("e{index:02}")));
            cumulative += 10;
            turn_usage(
                &mut store,
                &thread,
                Some(&id),
                "2026-01-02T12:00:00Z",
                10,
                cumulative,
            );
        }
    }
    let activity = read(&mut store, "2026-01-03T00:00:00Z", Range::Last24Hours).turn_activity;
    assert_eq!(activity.combinations, 10);
    assert_eq!(activity.series.len(), 8, "seven named plus one remainder");
    let folded = activity.series.last().unwrap();
    assert_eq!(
        (folded.key.as_str(), folded.label.as_str()),
        ("other:", "Other combinations (3)")
    );
    // Combinations with 3, 2 and 1 turns fold together.
    assert_eq!(folded.turns, 6);
    assert_eq!(folded.tokens.known_tokens.as_deref(), Some("60"));
    assert_eq!(
        activity.series.iter().map(|s| s.turns).sum::<u64>(),
        activity.total_turns
    );
    assert_eq!(activity.total_turns, 55);
    assert_eq!(folded.points.len(), 1, "the remainder merges bin by bin");
    assert_eq!(folded.points[0].turns, 6);
}

#[test]
fn turn_activity_counts_a_combination_s_sessions_across_every_bin_it_appears_in() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("turn-sessions.sqlite")).unwrap();
    // The same combination in three sessions, each an hour apart so no two of
    // its turns share a bin. A per-bin count would see one session at a time.
    for (index, hour) in ["09", "12", "15"].into_iter().enumerate() {
        let thread = format!("s{index}");
        session(&mut store, &thread);
        turn_context(&mut store, &thread, "t", "alpha", Some("high"));
        turn_usage(
            &mut store,
            &thread,
            Some("t"),
            &format!("2026-01-02T{hour}:00:00Z"),
            100,
            100,
        );
    }
    // A second combination sharing one of those sessions: distinct counts of
    // two combinations overlap, so a folded row cannot add them up.
    turn_context(&mut store, "s0", "u", "alpha", Some("low"));
    turn_usage(&mut store, "s0", Some("u"), "2026-01-02T10:00:00Z", 50, 150);

    let activity = read(&mut store, "2026-01-03T00:00:00Z", Range::Last24Hours).turn_activity;
    assert_eq!(
        activity
            .series
            .iter()
            .map(|series| (series.key.as_str(), series.turns, series.observed_sessions))
            .collect::<Vec<_>>(),
        [
            ("model:alpha|effort:high", 3, Some(3)),
            ("model:alpha|effort:low", 1, Some(1))
        ],
        "sessions are counted over the whole range, not within one bin"
    );
    assert_eq!(
        activity.series[0].points.len(),
        3,
        "the three turns still occupy three separate bins"
    );
}

#[test]
fn folded_rows_report_no_session_count_rather_than_a_wrong_one() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("fold-sessions.sqlite")).unwrap();
    // Ten combinations, all inside one session: their distinct counts overlap
    // completely, so summing or maximizing them would both mislead.
    session(&mut store, "s0");
    let mut cumulative = 0;
    for index in 0..10i64 {
        for turn in 0..=index {
            let id = format!("t{index}-{turn}");
            turn_context(&mut store, "s0", &id, "alpha", Some(&format!("e{index:02}")));
            cumulative += 10;
            turn_usage(&mut store, "s0", Some(&id), "2026-01-02T12:00:00Z", 10, cumulative);
        }
    }
    let activity = read(&mut store, "2026-01-03T00:00:00Z", Range::Last24Hours).turn_activity;
    let folded = activity.series.last().unwrap();
    assert_eq!(folded.kind, "other");
    assert_eq!(folded.observed_sessions, None);
    assert!(activity.series[..activity.series.len() - 1]
        .iter()
        .all(|series| series.observed_sessions == Some(1)));
}
