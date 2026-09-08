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
