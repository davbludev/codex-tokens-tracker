use super::{add, price, settle, Store};
use crate::aggregates::{
    session_list::{Page, Query, Sort},
    Data, Query as AggregateQuery, ReadError,
};

fn query() -> Query {
    Query {
        limit: 2,
        offset: 0,
        sort: Sort::Tokens,
        search: None,
        project: None,
        model: None,
        from_seconds: None,
        before_seconds: None,
    }
}
fn read(store: &mut Store, query: Query) -> Page {
    let Data::SessionList(page) = store
        .aggregates(AggregateQuery::SessionList { query })
        .unwrap()
        .data
    else {
        panic!()
    };
    page
}

#[test]
fn session_list_filters_pages_exact_cost_unknowns_and_live_changes() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("sessions.sqlite")).unwrap();
    add(
        &mut store,
        "a",
        None,
        100,
        Some("alpha"),
        Some("C:/project"),
    );
    add(
        &mut store,
        "b",
        Some("a"),
        200,
        Some("beta"),
        Some("C:/project"),
    );
    add(
        &mut store,
        "c",
        None,
        200,
        Some("alpha"),
        Some("C:/project"),
    );
    add(&mut store, "unknown", None, 300, None, None);
    settle(&mut store);
    let first = read(&mut store, query());
    assert_eq!(first.total_items, 4);
    assert_eq!(
        first
            .items
            .iter()
            .map(|r| r.thread_id.as_str())
            .collect::<Vec<_>>(),
        ["b", "c"]
    );
    assert_eq!(first.next_offset, Some(2));
    let second = read(
        &mut store,
        Query {
            offset: 2,
            ..query()
        },
    );
    assert_eq!(
        second
            .items
            .iter()
            .map(|r| r.thread_id.as_str())
            .collect::<Vec<_>>(),
        ["a", "unknown"]
    );
    assert_eq!(second.items[0].direct_subagent_count, Some(1));
    assert_eq!(
        second.items[0]
            .direct
            .tokens
            .total_tokens
            .known_tokens
            .as_deref(),
        Some("100")
    );
    assert!(second.items[1].unknown_model);
    assert_eq!(second.items[1].title, None);
    assert_eq!(second.items[1].duration_seconds, None);
    assert_eq!(second.items[1].weekly_percentage_impact, None);
    assert!(!serde_json::to_string(&second)
        .unwrap()
        .contains("FORBIDDEN_CONTENT"));
    let timestamp = time::OffsetDateTime::parse(
        first.items[0].last_observed_at.as_ref().unwrap(),
        &time::format_description::well_known::Rfc3339,
    )
    .unwrap()
    .unix_timestamp();
    let combined = read(
        &mut store,
        Query {
            search: Some("C".into()),
            project: Some("PROJECT".into()),
            model: Some("ALPHA".into()),
            from_seconds: Some(timestamp),
            before_seconds: Some(timestamp + 1),
            ..query()
        },
    );
    assert_eq!(combined.total_items, 1);
    assert_eq!(combined.items[0].thread_id, "c");
    assert_eq!(
        read(
            &mut store,
            Query {
                from_seconds: Some(timestamp + 1),
                ..query()
            }
        )
        .total_items,
        0
    );
    assert_eq!(
        read(
            &mut store,
            Query {
                before_seconds: Some(timestamp),
                ..query()
            }
        )
        .total_items,
        0
    );
    assert_eq!(
        read(
            &mut store,
            Query {
                search: Some("%".into()),
                ..query()
            }
        )
        .total_items,
        0,
        "search treats SQL wildcards literally"
    );
    assert_eq!(
        read(
            &mut store,
            Query {
                project: Some("Unavailable".into()),
                model: Some("Unavailable".into()),
                ..query()
            }
        )
        .items[0]
            .thread_id,
        "unknown"
    );
    // A complete zero cost ranks before unavailable cost; very large canonical
    // values remain exact instead of entering SQLite floating-point ordering.
    price(&mut store, "alpha", "999999999999.999999", 0);
    price(&mut store, "beta", "0", 0);
    let usd = read(
        &mut store,
        Query {
            limit: 50,
            sort: Sort::Usd,
            ..query()
        },
    );
    assert_eq!(
        usd.items
            .iter()
            .map(|r| r.thread_id.as_str())
            .collect::<Vec<_>>(),
        ["c", "a", "b", "unknown"]
    );
    assert_eq!(
        usd.items[2].direct.estimated_cost.known_subtotal.as_deref(),
        Some("0")
    );
    assert_eq!(usd.items[3].direct.estimated_cost.known_subtotal, None);
    add(
        &mut store,
        "d",
        None,
        400,
        Some("alpha"),
        Some("C:/project"),
    );
    settle(&mut store);
    assert_eq!(read(&mut store, query()).items[0].thread_id, "d");
    assert_eq!(
        store
            .aggregates(AggregateQuery::SessionList {
                query: Query {
                    limit: 51,
                    ..query()
                }
            })
            .unwrap_err(),
        ReadError::InvalidQuery
    );
    assert_eq!(
        store
            .aggregates(AggregateQuery::SessionList {
                query: Query {
                    from_seconds: Some(2),
                    before_seconds: Some(1),
                    ..query()
                }
            })
            .unwrap_err(),
        ReadError::InvalidQuery
    );
}

#[test]
fn session_list_thousands_are_bounded_and_metadata_only_sessions_stay_unavailable() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("many.sqlite")).unwrap();
    for index in 0..2001 {
        super::super::record_in_store(
            &mut store,
            &format!("s{index:04}"),
            &serde_json::json!({"type":"session_meta","payload":{"id":format!("s{index:04}")}}),
        );
    }
    let page = read(
        &mut store,
        Query {
            limit: 50,
            sort: Sort::Newest,
            ..query()
        },
    );
    assert_eq!(page.items.len(), 50);
    assert_eq!(page.total_items, 2001);
    assert_eq!(page.items[0].thread_id, "s0000");
    assert_eq!(page.items[0].last_observed_at, None);
    assert_eq!(page.items[0].direct.tokens.total_tokens.known_tokens, None);
    assert_eq!(
        page.items[0].direct_subagent_count, None,
        "pending hierarchy is not zero"
    );
    let tail = read(
        &mut store,
        Query {
            limit: 50,
            offset: 2000,
            sort: Sort::Newest,
            ..query()
        },
    );
    assert_eq!(tail.items.len(), 1);
    assert_eq!(tail.next_offset, None);
}

#[test]
fn session_list_orders_native_precision_and_bounds_models() {
    use super::super::record_in_store;
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("precision.sqlite")).unwrap();
    add(&mut store, "a-low", None, 100, Some("low"), None);
    add(&mut store, "z-high", None, 100, Some("high"), None);
    let mut usage = add(&mut store, "many-models", None, 100, Some("model-0"), None);
    for index in 1..12 {
        let turn = format!("turn-{index}");
        record_in_store(
            &mut store,
            "many-models",
            &serde_json::json!({"type":"turn_context","payload":{"turn_id":turn,"model":format!("model-{index}")}}),
        );
        usage["payload"]["turn_id"] = turn.clone().into();
        usage["payload"]["response_id"] = turn.into();
        usage["timestamp"] = format!("2026-01-01T00:00:{:02}Z", index + 1).into();
        let totals = usage["payload"]["usage"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(key, value)| {
                (
                    key.clone(),
                    serde_json::json!(value.as_i64().unwrap() * (index + 1)),
                )
            })
            .collect::<serde_json::Map<_, _>>();
        usage["payload"]["thread_token_usage"] = totals.into();
        record_in_store(&mut store, "many-models", &usage);
    }
    settle(&mut store);
    price(&mut store, "low", "999999999999.999998", 0);
    price(&mut store, "high", "999999999999.999999", 0);
    let page = read(
        &mut store,
        Query {
            limit: 50,
            sort: Sort::Usd,
            ..query()
        },
    );
    assert_eq!(
        page.items
            .iter()
            .map(|r| r.thread_id.as_str())
            .collect::<Vec<_>>(),
        ["z-high", "a-low", "many-models"]
    );
    assert_eq!(page.items[2].model_count, 12);
    assert_eq!(page.items[2].models.len(), 8);
    price(&mut store, "model-0", "999999999999.999999", 0);
    let partial = read(
        &mut store,
        Query {
            limit: 50,
            sort: Sort::Usd,
            ..query()
        },
    );
    assert_eq!(partial.items[2].thread_id, "many-models");
    assert!(partial.items[2]
        .direct
        .estimated_cost
        .known_subtotal
        .is_some());
    assert!(!partial.items[2].direct.estimated_cost.complete);
    // Timestamp spellings can differ; ordering uses native seconds/nanoseconds.
    store.connection().execute("UPDATE observations SET time_seconds=100,time_nanos=1,timestamp='1970-01-01T00:01:40.000000001Z' WHERE thread_id='z-high'", []).unwrap();
    store.connection().execute("UPDATE observations SET time_seconds=100,time_nanos=0,timestamp='1970-01-01T00:01:40Z' WHERE thread_id!='z-high'", []).unwrap();
    let newest = read(
        &mut store,
        Query {
            limit: 1,
            sort: Sort::Newest,
            ..query()
        },
    );
    assert_eq!(newest.items[0].thread_id, "z-high");
    let next = read(
        &mut store,
        Query {
            limit: 1,
            offset: 1,
            sort: Sort::Newest,
            ..query()
        },
    );
    assert_eq!(next.items[0].thread_id, "a-low");
    let wire = Store::read_aggregates(
        &temp.path().join("precision.sqlite"),
        AggregateQuery::SessionList { query: query() },
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(wire).unwrap()["data"]["kind"],
        "sessionList"
    );
}
