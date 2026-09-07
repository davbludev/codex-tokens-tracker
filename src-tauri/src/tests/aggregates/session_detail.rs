use super::*;
use crate::aggregates::session_detail::{BoundaryKind, Models, ShareUnavailable, Timeline};

/// Distinct turns with explicit deltas and a matching thread endpoint.
pub(super) fn observation(
    store: &mut Store,
    thread: &str,
    index: i64,
    tokens: i64,
    cumulative: i64,
    timestamp: &str,
    model: Option<&str>,
) {
    if index == 1 {
        record_in_store(
            store,
            thread,
            &serde_json::json!({"type":"session_meta","payload":{"id":thread}}),
        );
    }
    let turn = format!("{thread}-{index}");
    if let Some(model) = model {
        record_in_store(
            store,
            thread,
            &serde_json::json!({"type":"turn_context","payload":{"turn_id":turn,"model":model}}),
        );
    }
    let usage = serde_json::json!({"input_tokens":tokens-1,"cached_input_tokens":tokens-2,"cache_write_input_tokens":0,"output_tokens":1,"reasoning_output_tokens":1,"total_tokens":tokens});
    let endpoint = serde_json::json!({"input_tokens":cumulative-index,"cached_input_tokens":cumulative-2*index,"cache_write_input_tokens":0,"output_tokens":index,"reasoning_output_tokens":index,"total_tokens":cumulative});
    record_in_store(
        store,
        thread,
        &serde_json::json!({"timestamp":timestamp,"type":"token_usage_record","payload":{
            "thread_id":thread,"session_id":thread,"turn_id":turn,"response_id":turn,
            "usage":usage,"turn_token_usage":usage,"thread_token_usage":endpoint
        }}),
    );
}

fn models(store: &mut Store, thread: &str, after: Option<String>, limit: u32) -> Models {
    let Data::SessionModels(Some(value)) = store
        .aggregates(Query::SessionModels {
            thread: thread.into(),
            page: page(limit, after),
        })
        .unwrap()
        .data
    else {
        panic!()
    };
    value
}
fn timeline(store: &mut Store, thread: &str, point_budget: Option<u32>) -> Timeline {
    let Data::SessionTimeline(Some(value)) = store
        .aggregates(Query::SessionTimeline {
            thread: thread.into(),
            point_budget,
        })
        .unwrap()
        .data
    else {
        panic!()
    };
    value
}

#[test]
fn session_detail_models_conserve_categories_shares_and_retained_unknown_valuations() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("models.sqlite");
    let mut store = Store::open(&db).unwrap();
    observation(
        &mut store,
        "root",
        1,
        2,
        2,
        "2026-01-01T00:00:01Z",
        Some("alpha"),
    );
    observation(
        &mut store,
        "root",
        2,
        62,
        64,
        "2026-01-01T00:00:02Z",
        Some("beta"),
    );
    add(&mut store, "child", Some("root"), 100, Some("alpha"), None);
    add(
        &mut store,
        "grandchild",
        Some("child"),
        10,
        Some("beta"),
        None,
    );
    settle(&mut store);
    price(&mut store, "alpha", "0.000001", 0);
    price(&mut store, "beta", "0.000001", 0);
    let first = models(&mut store, "root", None, 1);
    assert_eq!(first.scope, "direct");
    assert_eq!(first.total_items, 2);
    assert_eq!(cost(&first.direct), (Some("64"), true));
    assert_eq!(first.items[0].attribution.id, "model:alpha");
    assert_eq!(first.items[0].cost_share.as_deref(), Some("3.13")); // 2/64 = 3.125%, half-up.
    assert!(first.items[0].cost_share_unavailable_reason.is_none());
    let second = models(&mut store, "root", first.next_cursor, 1);
    assert_eq!(cost(&second.direct), (Some("64"), true));
    assert_eq!(second.items[0].cost_share.as_deref(), Some("96.88"));
    assert!(second.next_cursor.is_none());
    let mut sums = [0i64; 6];
    for item in [&first.items[0], &second.items[0]] {
        let t = &item.direct.tokens;
        for (sum, category) in sums.iter_mut().zip([
            &t.total_tokens,
            &t.input_tokens,
            &t.cached_input_tokens,
            &t.cache_write_tokens,
            &t.output_tokens,
            &t.reasoning_tokens,
        ]) {
            assert!(category.complete);
            *sum += category
                .known_tokens
                .as_ref()
                .unwrap()
                .parse::<i64>()
                .unwrap();
        }
    }
    assert_eq!(sums, [64, 62, 60, 0, 2, 2]);
    assert_eq!(
        total(session(&mut store, "root").inclusive.as_ref().unwrap()),
        Some("174")
    );
    // Later attribution conflict retains the original immutable valuation.
    price(&mut store, "alpha", "999", 1);
    record_in_store(
        &mut store,
        "root",
        &serde_json::json!({"type":"turn_context","payload":{"turn_id":"root-1","model":"conflicting"}}),
    );
    drop(store);
    let mut store = Store::open(&db).unwrap();
    let rows = models(&mut store, "root", None, 50);
    let unknown = rows
        .items
        .iter()
        .find(|row| row.attribution.id == "unknown:")
        .unwrap();
    assert_eq!(cost(&unknown.direct), (Some("2"), true));
    assert_eq!(unknown.cost_share.as_deref(), Some("3.13"));
    assert!(unknown.direct.coverage.unknown_model);
    observation(&mut store, "root", 3, 4, 68, "2026-01-01T00:00:03Z", None);
    let rows = models(&mut store, "root", None, 50);
    assert_eq!(cost(&rows.direct), (Some("64"), false));
    assert_eq!(total(&rows.direct), Some("68"));
    assert!(rows.items.iter().all(|row| row.cost_share.is_none()
        && row.cost_share_unavailable_reason == Some(ShareUnavailable::Incomplete)));
}

#[test]
fn session_detail_zero_unavailable_and_large_exact_shares() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("shares.sqlite")).unwrap();
    add(&mut store, "free", None, 2, Some("zero"), None);
    price(&mut store, "zero", "0", 0);
    let zero = models(&mut store, "free", None, 50);
    assert_eq!(cost(&zero.direct), (Some("0"), true));
    assert_eq!(
        zero.items[0].cost_share_unavailable_reason,
        Some(ShareUnavailable::ZeroDenominator)
    );
    add(&mut store, "unpriced", None, 2, None, None);
    let unavailable = models(&mut store, "unpriced", None, 50);
    assert_eq!(
        unavailable.items[0].cost_share_unavailable_reason,
        Some(ShareUnavailable::Unavailable)
    );
    // A valid maximum subtotal must remain usable when percentage scaling would overflow i128.
    use crate::aggregates::session_detail::cost_share;
    let maximum = EstimatedCost {
        known_subtotal: Some(i128::MAX.to_string()),
        complete: true,
    };
    assert_eq!(
        cost_share(&maximum, &maximum).unwrap(),
        (Some("100.00".into()), None)
    );
    let zero = EstimatedCost {
        known_subtotal: Some("0".into()),
        complete: true,
    };
    assert_eq!(
        cost_share(&zero, &maximum).unwrap(),
        (Some("0.00".into()), None)
    );
}

#[test]
fn session_detail_metadata_readiness_missing_sessions_and_live_snapshots() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("metadata.sqlite")).unwrap();
    add(
        &mut store,
        "child",
        Some("missing"),
        30,
        Some("alpha"),
        None,
    );
    let detail = session(&mut store, "child");
    assert!(
        detail.title.is_none()
            && detail.started_at.is_none()
            && detail.ended_at.is_none()
            && detail.duration_seconds.is_none()
    );
    assert_eq!(
        detail.first_observed_at.as_deref(),
        Some("2026-01-01T00:00:01Z")
    );
    assert_eq!(detail.first_observed_at, detail.last_observed_at);
    assert!(detail.inclusive.is_none());
    assert_eq!(
        total(&models(&mut store, "child", None, 50).direct),
        Some("30")
    );
    assert_eq!(timeline(&mut store, "child", None).point_budget, 512);
    for query in [
        Query::SessionModels {
            thread: "absent".into(),
            page: page(50, None),
        },
        Query::SessionTimeline {
            thread: "absent".into(),
            point_budget: None,
        },
    ] {
        assert!(matches!(
            store.aggregates(query).unwrap().data,
            Data::SessionModels(None) | Data::SessionTimeline(None)
        ));
    }
    settle(&mut store);
    let missing = session(&mut store, "missing");
    assert!(
        missing.placeholder
            && missing.first_observed_at.is_none()
            && missing.last_observed_at.is_none()
    );
    assert_eq!(total(missing.inclusive.as_ref().unwrap()), Some("30"));
    let empty = timeline(&mut store, "missing", None);
    assert!(empty.points.is_empty() && empty.first_observed_at.is_none());
    assert_eq!(empty.source_observation_count, 0);
    observation(&mut store, "live", 1, 2, 2, "2026-01-01T00:00:02Z", None);
    let before = timeline(&mut store, "live", None);
    observation(&mut store, "live", 2, 2, 4, "2026-01-01T00:00:03Z", None);
    let after = timeline(&mut store, "live", None);
    assert_eq!(total(&before.direct), Some("2"));
    assert_eq!(total(&after.direct), Some("4"));
    assert_eq!(
        session(&mut store, "live").last_observed_at.as_deref(),
        Some("2026-01-01T00:00:03Z")
    );
    for budget in [0, 7, 4097, u32::MAX] {
        assert!(matches!(
            store.aggregates(Query::SessionTimeline {
                thread: "live".into(),
                point_budget: Some(budget)
            }),
            Err(ReadError::InvalidQuery)
        ));
    }
    for limit in [0, 51, u32::MAX] {
        assert!(matches!(
            store.aggregates(Query::SessionModels {
                thread: "live".into(),
                page: page(limit, None)
            }),
            Err(ReadError::InvalidQuery)
        ));
    }
    let query: Query = serde_json::from_value(
        serde_json::json!({"kind":"sessionTimeline","thread":"live","pointBudget":8}),
    )
    .unwrap();
    let wire = serde_json::to_value(store.aggregates(query).unwrap()).unwrap();
    assert_eq!(wire["data"]["kind"], "sessionTimeline");
    assert_eq!(wire["data"]["data"]["pointBudget"], 8);
    assert_eq!(wire["data"]["data"]["scope"], "direct");
    assert!(wire["data"]["data"]["points"][0].get("segments").is_none());
}

#[test]
fn session_detail_timeline_groups_equal_nanoseconds_and_excludes_untimed_rejected_usage() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("time.sqlite")).unwrap();
    for (index, time) in [
        (1, "2026-01-01T00:00:01.000000001Z"),
        (2, "2026-01-01T00:00:01.000000001Z"),
        (3, "2026-01-01T00:00:02.000000002Z"),
        (4, "2026-01-01T00:00:03Z"),
        (5, "2026-01-01T00:00:04Z"),
    ] {
        observation(&mut store, "root", index, 2, 2 * index, time, Some("alpha"));
    }
    price(&mut store, "alpha", "0.000001", 0);
    // Compatibility fixtures exercise accepted untimed history and missing totals.
    store.connection().execute("UPDATE observations SET time_seconds=NULL,time_nanos=NULL WHERE thread_id='root' AND timestamp='2026-01-01T00:00:03Z'", []).unwrap();
    store
        .connection()
        .execute(
            "UPDATE observations SET accepted=0,total=NULL WHERE thread_id='root' AND timestamp='2026-01-01T00:00:04Z'",
            [],
        )
        .unwrap();
    let chart = timeline(&mut store, "root", Some(8));
    assert_eq!(chart.source_observation_count, 3);
    assert_eq!(chart.source_point_count, 2);
    assert_eq!(chart.returned_point_count, 2);
    assert_eq!(chart.untimed_observation_count, 1);
    assert_eq!(total(&chart.direct), Some("8"));
    assert_eq!(cost(&chart.direct), (Some("8"), true));
    assert!(chart.direct.coverage.unresolved_usage);
    assert_eq!(chart.points[0].time.nanos, 1);
    assert_eq!(chart.points[1].time.nanos, 2);
    assert_eq!(
        chart.points[0]
            .cumulative_total_tokens
            .known_tokens
            .as_deref(),
        Some("4")
    );
    assert_eq!(
        chart.points[1]
            .cumulative_total_tokens
            .known_tokens
            .as_deref(),
        Some("6")
    );
    assert_eq!(
        chart.points[1]
            .cumulative_estimated_cost
            .known_subtotal
            .as_deref(),
        Some("6")
    );
    assert!(
        !chart.points[0].tokens_connect_from_previous
            && !chart.points[0].cost_connect_from_previous
    );
    assert!(
        chart.points[1].tokens_connect_from_previous && chart.points[1].cost_connect_from_previous
    );
    let detail = session(&mut store, "root");
    assert_eq!(
        detail.first_observed_at.as_deref(),
        Some("2026-01-01T00:00:01.000000001Z")
    );
    assert_eq!(
        detail.last_observed_at.as_deref(),
        Some("2026-01-01T00:00:02.000000002Z")
    );
}

#[test]
fn session_detail_timeline_bounds_preserve_hidden_gaps_and_exact_endpoints() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("bounded.sqlite")).unwrap();
    for index in 1..=80 {
        observation(
            &mut store,
            "root",
            index,
            2,
            2 * index,
            &format!("2026-01-01T00:{:02}:{:02}Z", index / 60, index % 60),
            Some(if index == 7 || index == 9 {
                "unpriced"
            } else {
                "alpha"
            }),
        );
    }
    price(&mut store, "alpha", "0.000001", 0);
    store
        .connection()
        .execute(
            "UPDATE observations SET total=NULL WHERE thread_id='root' AND timestamp='2026-01-01T00:00:48Z'",
            [],
        )
        .unwrap();
    let chart = timeline(&mut store, "root", Some(8));
    assert_eq!(chart.source_observation_count, 80);
    assert_eq!(chart.source_point_count, 80);
    assert!(chart.returned_point_count <= 8 && chart.boundaries.len() <= 4);
    assert_eq!(
        chart.points.first().unwrap().time,
        chart.first_observed_at.unwrap()
    );
    assert_eq!(
        chart.points.last().unwrap().time,
        chart.last_observed_at.unwrap()
    );
    assert!(chart
        .points
        .windows(2)
        .all(|pair| pair[0].time < pair[1].time));
    let final_point = chart.points.last().unwrap();
    assert_eq!(
        final_point.cumulative_total_tokens.known_tokens.as_deref(),
        Some("158")
    );
    assert!(!final_point.cumulative_total_tokens.complete);
    assert_eq!(
        final_point
            .cumulative_estimated_cost
            .known_subtotal
            .as_deref(),
        Some("156")
    );
    assert!(!final_point.cumulative_estimated_cost.complete);
    assert!(chart.boundaries.iter().any(|b| b.overloaded
        && b.kinds.contains(&BoundaryKind::UnpricedUsage)
        && b.kinds.contains(&BoundaryKind::PricedUsageResumed)));
    assert!(chart.boundaries.iter().any(|b| b.overloaded
        && b.kinds.contains(&BoundaryKind::MissingTokens)
        && b.kinds.contains(&BoundaryKind::TokensResumed)));
    // Both first-bin endpoints look priced: hidden unpriced observations still disconnect them.
    assert!(!chart.points[1].cost_connect_from_previous);
    assert!(!chart.points[2].cost_connect_from_previous);
    // At full resolution only the cost series breaks at the unpriced observations.
    let full = timeline(&mut store, "root", Some(4096));
    assert_eq!(full.returned_point_count, 80);
    assert!(full.points[6].tokens_connect_from_previous);
    assert!(
        !full.points[6].cost_connect_from_previous && !full.points[7].cost_connect_from_previous
    );
    assert!(full.points[10].cost_connect_from_previous);
    assert!(
        !full.points[47].tokens_connect_from_previous && full.points[47].cost_connect_from_previous
    );
    assert!(
        !full.points[48].tokens_connect_from_previous
            && full.points[49].tokens_connect_from_previous
    );
}

#[test]
fn session_detail_model_pages_are_bounded_with_a_whole_session_denominator() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("model-pages.sqlite")).unwrap();
    for index in 1..=53 {
        observation(
            &mut store,
            "root",
            index,
            2,
            index * 2,
            "2026-01-01T00:00:01Z",
            Some(&format!("model-{index:03}")),
        );
    }
    let first = models(&mut store, "root", None, 50);
    assert_eq!(first.items.len(), 50);
    assert_eq!(first.total_items, 53);
    assert_eq!(total(&first.direct), Some("106"));
    let second = models(&mut store, "root", first.next_cursor, 50);
    assert_eq!(second.items.len(), 3);
    assert_eq!(second.items[0].attribution.id, "model:model-051");
    assert_eq!(total(&second.direct), Some("106"));
    assert!(second.next_cursor.is_none());
}
