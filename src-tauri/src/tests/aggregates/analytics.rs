use super::session_detail::observation;
use super::*;
use crate::{
    aggregates::analytics as a,
    weekly::{Time, Unavailable},
};

fn time(seconds: i64) -> Time {
    Time { seconds, nanos: 0 }
}
fn projects(store: &mut Store) -> a::Projects {
    let Data::ProjectAnalytics(p) = store
        .aggregates(Query::ProjectAnalytics {
            page: page(50, None),
        })
        .unwrap()
        .data
    else {
        panic!()
    };
    p
}
fn models(store: &mut Store, now: i64) -> a::Models {
    let Data::ModelAnalytics(p) = store
        .aggregates_at(
            Query::ModelAnalytics {
                page: page(50, None),
            },
            time(now),
        )
        .unwrap()
        .data
    else {
        panic!()
    };
    p
}
fn history(store: &mut Store, start: Time, end: Time, budget: Option<u32>) -> a::History {
    let Data::ModelHistory(h) = store
        .aggregates(Query::ModelHistory {
            model: "model:alpha".into(),
            start,
            end,
            point_budget: budget,
        })
        .unwrap()
        .data
    else {
        panic!()
    };
    h
}
fn weekly(store: &mut Store, timestamp: &str, percent: u32) {
    record_in_store(
        store,
        "limits",
        &serde_json::json!({"type":"event_msg","timestamp":timestamp,"payload":{"type":"token_count","rate_limits":{"limit_id":"codex","secondary":{"used_percent":percent,"window_minutes":10080}}}}),
    );
}

#[test]
fn analytics_conserves_direct_usage_overlap_classification_and_average() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("analytics.sqlite")).unwrap();
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
        6,
        8,
        "2026-01-01T00:00:02Z",
        Some("beta"),
    );
    add(&mut store, "child", Some("root"), 4, Some("alpha"), None);
    price(&mut store, "alpha", "0.000001", 0);
    price(&mut store, "beta", "0.000001", 0);
    let pending = projects(&mut store);
    assert!(pending.items[0].classification.is_none());
    assert_eq!(total(&pending.items[0].direct), Some("12"));
    settle(&mut store);
    let p = projects(&mut store);
    let row = &p.items[0];
    assert_eq!(row.direct.observed_sessions, 2);
    assert_eq!(cost(&row.direct), (Some("12"), true));
    assert_eq!(row.average_session_cost.amount.as_deref(), Some("6"));
    let classification = row.classification.as_ref().unwrap();
    assert_eq!(total(&classification.proven_subagent), Some("4"));
    assert_eq!(
        total(&classification.parent_classification_unavailable),
        Some("8")
    );
    let m = models(&mut store, 1);
    assert_eq!(m.items.len(), 2);
    assert_eq!(m.items[0].accepted_usage_events, 2);
    assert_eq!(m.items[0].sessions_used, 2);
    assert_eq!(m.items[1].sessions_used, 1); // Shared root is intentionally in both models.
    assert_eq!(m.items[0].cost_share.as_deref(), Some("50.00"));
    assert_eq!(m.items[1].cost_share.as_deref(), Some("50.00"));
    assert_eq!(total(&global(&mut store)), Some("12"));
    record_in_store(
        &mut store,
        "empty",
        &serde_json::json!({"type":"session_meta","payload":{"id":"empty"}}),
    );
    let p = projects(&mut store);
    assert_eq!(p.items[0].direct.observed_sessions, 3);
    assert_eq!(
        p.items[0].average_session_cost.unavailable_reason,
        Some("sessionsWithoutAcceptedUsage")
    );
    assert_eq!(cost(&p.items[0].direct), (Some("12"), true));
}

#[test]
fn analytics_catalog_unknown_immutable_cost_as_of_prices_and_unpriced_shares() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("catalog.sqlite")).unwrap();
    add(&mut store, "known", None, 3, Some("alpha"), None);
    add(&mut store, "child", Some("missing"), 7, Some("beta"), None);
    price(&mut store, "alpha", "0.000001", 0);
    price(&mut store, "beta", "0.000001", 0);
    price(&mut store, "alpha", "999", 10);
    record_in_store(
        &mut store,
        "known",
        &serde_json::json!({"type":"turn_context","payload":{"turn_id":"known","model":"unused"}}),
    );
    settle(&mut store);
    let before = models(&mut store, -1);
    assert!(before
        .items
        .iter()
        .all(|m| m.active_pricing_version.is_none()));
    let m = models(&mut store, 9);
    let alpha = m
        .items
        .iter()
        .find(|m| m.attribution.id == "model:alpha")
        .unwrap();
    assert_eq!(alpha.accepted_usage_events, 0);
    assert_eq!(alpha.sessions_used, 0);
    assert_eq!(total(&alpha.direct), None);
    assert_eq!(
        alpha.active_pricing_version.as_ref().unwrap().effective_at,
        time(0)
    );
    let unknown = m
        .items
        .iter()
        .find(|m| m.attribution.id == "unknown:")
        .unwrap();
    assert_eq!(cost(&unknown.direct), (Some("3"), true));
    assert_eq!(unknown.cost_share.as_deref(), Some("30.00"));
    assert!(unknown.active_pricing_version.is_none());
    let after = models(&mut store, 10);
    assert_eq!(
        after
            .items
            .iter()
            .find(|m| m.attribution.id == "model:alpha")
            .unwrap()
            .active_pricing_version
            .as_ref()
            .unwrap()
            .effective_at,
        time(10)
    );
    let p = projects(&mut store);
    assert_eq!(p.items[0].direct.observed_sessions, 2); // Missing parent is excluded.
    assert_eq!(p.items[0].average_session_cost.amount.as_deref(), Some("5"));
    add(&mut store, "unpriced", None, 2, None, None);
    let m = models(&mut store, 10);
    assert_eq!(cost(&m.direct), (Some("10"), false));
    assert!(m.items.iter().all(|m| m.cost_share.is_none()));
    assert_eq!(
        projects(&mut store).items[0]
            .average_session_cost
            .unavailable_reason,
        Some("incompleteCost")
    );
}

#[test]
fn analytics_cycle_intervals_exclude_start_include_end_reset_and_ambiguity() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("cycle.sqlite")).unwrap();
    for i in 1..=4 {
        observation(
            &mut store,
            "root",
            i,
            2,
            i * 2,
            &format!("2026-01-01T00:00:0{i}Z"),
            Some("alpha"),
        );
    }
    price(&mut store, "alpha", "0.000001", 0);
    assert!(projects(&mut store).items[0].current_cycle.direct.is_none());
    weekly(&mut store, "2026-01-01T00:00:01Z", 40);
    weekly(&mut store, "2026-01-01T00:00:03Z", 40); // Ratio suppressed, measurable usage retained.
    let p = projects(&mut store);
    let cycle = &p.items[0].current_cycle;
    assert!(cycle.partial && cycle.unavailable_reason.is_none());
    assert_eq!(cost(cycle.direct.as_ref().unwrap()), (Some("4"), true));
    assert_eq!(total(cycle.direct.as_ref().unwrap()), Some("4"));
    assert_eq!(cycle.end.unwrap().seconds - cycle.start.unwrap().seconds, 2);
    weekly(&mut store, "2026-01-01T00:00:04Z", 1);
    assert!(projects(&mut store).items[0].current_cycle.direct.is_none());
    weekly(&mut store, "2026-01-01T00:00:05Z", 2);
    let p = projects(&mut store);
    assert_eq!(p.items[0].current_cycle.start.unwrap().seconds, 1767225604);
    assert_eq!(
        total(p.items[0].current_cycle.direct.as_ref().unwrap()),
        None
    );
    weekly(&mut store, "2026-01-01T00:00:05Z", 3);
    let p = projects(&mut store);
    assert!(p.items[0].current_cycle.has_ambiguous_observations);
    assert_eq!(
        p.items[0].current_cycle.unavailable_reason,
        Some(Unavailable::AmbiguousObservation)
    );
    assert!(p.items[0].current_cycle.direct.is_none());
}

#[test]
fn analytics_history_sql_bins_exact_bounds_gaps_untimed_and_completeness() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("history.sqlite")).unwrap();
    for i in 1..=5 {
        observation(
            &mut store,
            "root",
            i,
            2,
            i * 2,
            &format!("2026-01-01T00:00:0{i}Z"),
            Some("alpha"),
        );
    }
    store
        .connection()
        .execute(
            "UPDATE observations SET time_nanos=NULL WHERE timestamp='2026-01-01T00:00:05Z'",
            [],
        )
        .unwrap();
    store.connection().execute("UPDATE observations SET normalized=json_remove(normalized,'$.usage.cached_input_tokens') WHERE timestamp='2026-01-01T00:00:03Z'",[]).unwrap();
    price(&mut store, "alpha", "0.000001", 0);
    let h = history(&mut store, time(1767225601), time(1767225604), Some(2));
    assert_eq!(h.bins.len(), 2);
    assert_eq!(h.untimed_accepted_usage_events, 1);
    assert_eq!(h.bins[0].accepted_usage_events, 1);
    assert_eq!(
        h.bins[0].end,
        Time {
            seconds: 1767225602,
            nanos: 500_000_000
        }
    );
    assert_eq!(h.bins[1].accepted_usage_events, 2);
    assert_eq!(
        h.bins[1].tokens.total_tokens.known_tokens.as_deref(),
        Some("4")
    );
    assert_eq!(
        h.bins[1].estimated_cost.known_subtotal.as_deref(),
        Some("2")
    );
    assert!(!h.bins[1].estimated_cost.complete);
    assert!(!h.bins[1].tokens.cached_input_tokens.complete);
    let sparse = history(&mut store, time(1767225600), time(1767225610), None);
    assert_eq!(sparse.point_budget, 512);
    assert_eq!(sparse.bins.len(), 4);
    assert_eq!(
        sparse
            .bins
            .iter()
            .map(|b| b.accepted_usage_events)
            .sum::<u64>(),
        4
    );
    assert!(sparse.bins.iter().all(|b| b.start < b.end));
    let narrow = history(
        &mut store,
        time(1767225600),
        Time {
            seconds: 1767225600,
            nanos: 3,
        },
        Some(4096),
    );
    assert!(narrow.bins.is_empty());
    for budget in [0, 4097, u32::MAX] {
        assert!(matches!(
            store.aggregates(Query::ModelHistory {
                model: "model:alpha".into(),
                start: time(0),
                end: time(1),
                point_budget: Some(budget)
            }),
            Err(ReadError::InvalidQuery)
        ));
    }
    for (start, end) in [
        (time(1), time(1)),
        (time(2), time(1)),
        (
            Time {
                seconds: 0,
                nanos: 1_000_000_000,
            },
            time(1),
        ),
    ] {
        assert!(matches!(
            store.aggregates(Query::ModelHistory {
                model: "model:alpha".into(),
                start,
                end,
                point_budget: None
            }),
            Err(ReadError::InvalidQuery)
        ));
    }
}

#[test]
fn analytics_pages_and_project_model_previews_are_bounded_and_deterministic() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("pages.sqlite")).unwrap();
    for i in 1..=53 {
        observation(
            &mut store,
            "root",
            i,
            2,
            i * 2,
            "2026-01-01T00:00:01Z",
            Some(&format!("model-{i:03}")),
        );
    }
    let p = projects(&mut store);
    assert_eq!(p.items[0].models.items.len(), 5);
    assert_eq!(p.items[0].models.total_items, 53);
    let Data::ProjectModels(next) = store
        .aggregates(Query::ProjectModels {
            project: "unknown:".into(),
            page: page(50, p.items[0].models.next_cursor.clone()),
        })
        .unwrap()
        .data
    else {
        panic!()
    };
    assert_eq!(next.items.len(), 48);
    assert_eq!(next.items[0].id, "model:model-006");
    assert!(next.next_cursor.is_none());
    let first = models(&mut store, 1);
    assert_eq!(first.items.len(), 50);
    assert_eq!(first.total_items, 53);
    assert_eq!(total(&first.direct), Some("106"));
    let Data::ModelAnalytics(second) = store
        .aggregates(Query::ModelAnalytics {
            page: page(50, first.next_cursor),
        })
        .unwrap()
        .data
    else {
        panic!()
    };
    assert_eq!(second.items.len(), 3);
    assert_eq!(total(&second.direct), Some("106"));
    for limit in [0, 51] {
        for query in [
            Query::ProjectAnalytics {
                page: page(limit, None),
            },
            Query::ModelAnalytics {
                page: page(limit, None),
            },
            Query::ProjectModels {
                project: "unknown:".into(),
                page: page(limit, None),
            },
        ] {
            assert!(matches!(
                store.aggregates(query),
                Err(ReadError::InvalidQuery)
            ));
        }
    }
    let wire: Query = serde_json::from_value(serde_json::json!({"kind":"modelHistory","model":"model:model-001","start":{"seconds":0,"nanos":0},"end":{"seconds":2000000000,"nanos":0},"pointBudget":1})).unwrap();
    let value = serde_json::to_value(store.aggregates(wire).unwrap()).unwrap();
    assert_eq!(value["data"]["kind"], "modelHistory");
    assert_eq!(value["data"]["data"]["bins"][0]["acceptedUsageEvents"], 1);
}

#[test]
fn analytics_rounding_zero_cost_and_nanosecond_price_boundaries() {
    use crate::aggregates::session_detail::ShareUnavailable;
    use crate::pricing::{CacheWritePolicy, PriceInput, ReasoningPolicy};
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("precision.sqlite");
    let mut store = Store::open(&db).unwrap();
    add(&mut store, "a", None, 2, Some("alpha"), None);
    add(&mut store, "b", None, 3, Some("alpha"), None);
    price(&mut store, "alpha", "0.000001", 0);
    assert_eq!(
        projects(&mut store).items[0]
            .average_session_cost
            .amount
            .as_deref(),
        Some("3")
    ); // 5/2 trillionths, half-up.
    store
        .save_model_price_at(
            "alpha",
            PriceInput {
                input: "0".into(),
                cached_input: "0".into(),
                cache_write: "0".into(),
                output: "0".into(),
                reasoning: None,
                reasoning_policy: ReasoningPolicy::Included,
                cache_write_policy: CacheWritePolicy::Unknown,
            },
            false,
            (1, 7),
        )
        .unwrap();
    for (nanos, effective) in [
        (6, time(0)),
        (
            7,
            Time {
                seconds: 1,
                nanos: 7,
            },
        ),
    ] {
        let Data::ModelAnalytics(m) = store
            .aggregates_at(
                Query::ModelAnalytics {
                    page: page(50, None),
                },
                Time { seconds: 1, nanos },
            )
            .unwrap()
            .data
        else {
            panic!()
        };
        assert_eq!(
            m.items[0]
                .active_pricing_version
                .as_ref()
                .unwrap()
                .effective_at,
            effective
        );
        assert_eq!(cost(&m.items[0].direct), (Some("5"), true));
    }
    // New usage uses the zero version; historical valuation still determines the global share.
    add(
        &mut store,
        "zero",
        None,
        2,
        Some("zero"),
        Some("C:/zero-project"),
    );
    price(&mut store, "zero", "0", 0);
    let m = models(&mut store, 2);
    assert_eq!(
        m.items
            .iter()
            .find(|m| m.attribution.id == "model:zero")
            .unwrap()
            .cost_share
            .as_deref(),
        Some("0.00")
    );
    let Data::ProjectAnalytics(p) = Store::read_aggregates(
        &db,
        Query::ProjectAnalytics {
            page: page(1, None),
        },
    )
    .unwrap()
    .data
    else {
        panic!()
    };
    assert_eq!(p.items.len(), 1);
    assert_eq!(p.total_items, 2);
    assert_eq!(cost(&p.direct), (Some("5"), true));
    assert!(p.next_cursor.is_some());
    let mut zero_store = Store::open(&temp.path().join("zero.sqlite")).unwrap();
    add(&mut zero_store, "zero", None, 2, Some("zero"), None);
    price(&mut zero_store, "zero", "0", 0);
    assert_eq!(
        models(&mut zero_store, 1).items[0].cost_share_unavailable_reason,
        Some(ShareUnavailable::ZeroDenominator)
    );
}
