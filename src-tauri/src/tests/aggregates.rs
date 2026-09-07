use super::{historical_record, record_in_store, settle};
use crate::{aggregates::*, storage::Store};
mod session_list;

fn page(limit: u32, after: Option<String>) -> PageRequest {
    PageRequest { limit, after }
}

fn add(
    store: &mut Store,
    id: &str,
    parent: Option<&str>,
    total: i64,
    model: Option<&str>,
    cwd: Option<&str>,
) -> serde_json::Value {
    record_in_store(
        store,
        id,
        &serde_json::json!({"type":"session_meta","payload":{"id":id,"parent_thread_id":parent,"cwd":cwd,"prompt":"FORBIDDEN_CONTENT"}}),
    );
    if let Some(model) = model {
        record_in_store(
            store,
            id,
            &serde_json::json!({"type":"turn_context","payload":{"turn_id":id,"model":model}}),
        );
    }
    let mut usage = historical_record(1);
    usage["payload"]["thread_id"] = id.into();
    usage["payload"]["session_id"] = id.into();
    usage["payload"]["turn_id"] = id.into();
    usage["payload"]["response_id"] = id.into();
    let tokens = serde_json::json!({"input_tokens":total-1,"cached_input_tokens":total-2,"cache_write_input_tokens":0,"output_tokens":1,"reasoning_output_tokens":1,"total_tokens":total});
    usage["payload"]["usage"] = tokens.clone();
    usage["payload"]["thread_token_usage"] = tokens;
    record_in_store(store, id, &usage);
    usage
}

fn global(store: &mut Store) -> Summary {
    let Data::Global(value) = store.aggregates(Query::Global).unwrap().data else {
        panic!()
    };
    value
}
fn session(store: &mut Store, id: &str) -> Session {
    let Data::Session(Some(value)) = store
        .aggregates(Query::Session { thread: id.into() })
        .unwrap()
        .data
    else {
        panic!()
    };
    value
}
fn total(summary: &Summary) -> Option<&str> {
    summary.tokens.total_tokens.known_tokens.as_deref()
}

fn price(store: &mut Store, model: &str, rate: &str, seconds: i64) {
    use crate::pricing::{CacheWritePolicy, PriceInput, ReasoningPolicy};
    store
        .save_model_price_at(
            model,
            PriceInput {
                input: rate.into(),
                cached_input: rate.into(),
                cache_write: rate.into(),
                output: rate.into(),
                reasoning: None,
                reasoning_policy: ReasoningPolicy::Included,
                cache_write_policy: CacheWritePolicy::Unknown,
            },
            seconds == 0,
            (seconds, 0),
        )
        .unwrap();
    while store.pricing_work_pending().unwrap() {
        store.process_pricing_work().unwrap();
    }
}

fn cost(summary: &Summary) -> (Option<&str>, bool) {
    (
        summary.estimated_cost.known_subtotal.as_deref(),
        summary.estimated_cost.complete,
    )
}

#[test]
fn aggregates_cost_completeness_conserves_scopes_and_immutable_history() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("cost.sqlite");
    let mut store = Store::open(&db).unwrap();
    assert_eq!(cost(&global(&mut store)), (None, false));
    let root_record = add(
        &mut store,
        "root",
        None,
        100,
        Some("alpha"),
        Some("C:/cost-a"),
    );
    add(
        &mut store,
        "child",
        Some("root"),
        30,
        Some("beta"),
        Some("C:/cost-b"),
    );
    add(&mut store, "unknown", Some("child"), 5, None, None);
    settle(&mut store);
    assert_eq!(cost(&global(&mut store)), (None, false));
    // Equal category rates count input/output once, despite cached/reasoning overlap.
    price(&mut store, "alpha", "0.000001", 0);
    assert_eq!(
        cost(&session(&mut store, "root").direct),
        (Some("100"), true)
    );
    assert_eq!(
        cost(session(&mut store, "root").inclusive.as_ref().unwrap()),
        (Some("100"), false)
    );
    price(&mut store, "beta", "0", 0);
    assert_eq!(
        cost(&session(&mut store, "child").direct),
        (Some("0"), true)
    );
    assert_eq!(
        cost(session(&mut store, "child").inclusive.as_ref().unwrap()),
        (Some("0"), false)
    );
    assert_eq!(cost(&session(&mut store, "unknown").direct), (None, false));
    assert_eq!(cost(&global(&mut store)), (Some("100"), false));
    for models in [false, true] {
        let mut after = None;
        let mut costs = Vec::new();
        loop {
            let query = if models {
                Query::Models {
                    page: page(1, after),
                }
            } else {
                Query::Projects {
                    page: page(1, after),
                }
            };
            let groups = match store.aggregates(query).unwrap().data {
                Data::Projects(p) | Data::Models(p) => p,
                _ => panic!(),
            };
            assert_eq!(cost(&groups.direct), (Some("100"), false));
            costs.push((
                groups.items[0].direct.estimated_cost.known_subtotal.clone(),
                groups.items[0].direct.estimated_cost.complete,
            ));
            after = groups.next_cursor;
            if after.is_none() {
                break;
            }
        }
        costs.sort();
        assert_eq!(
            costs,
            vec![
                (None, false),
                (Some("0".into()), true),
                (Some("100".into()), true)
            ]
        );
    }
    for query in [
        Query::Sessions {
            page: page(1, None),
        },
        Query::Children {
            thread: "root".into(),
            page: page(1, None),
        },
        Query::Ancestors {
            thread: "unknown".into(),
            page: page(1, None),
        },
    ] {
        let Data::Sessions(p) = store.aggregates(query).unwrap().data else {
            panic!()
        };
        let expected = match p.total_items {
            3 => (Some("100"), false),
            2 => (Some("100"), true),
            1 => (Some("0"), true),
            _ => panic!(),
        };
        assert_eq!(cost(&p.direct), expected);
    }
    // An edit and model conflict must not revalue the previously priced observation.
    price(&mut store, "alpha", "999", 1);
    record_in_store(
        &mut store,
        "root",
        &serde_json::json!({"type":"turn_context","payload":{"turn_id":"root","model":"conflicting"}}),
    );
    drop(store);
    let mut store = Store::open(&db).unwrap();
    record_in_store(&mut store, "replay", &root_record);
    settle(&mut store);
    let Data::Models(groups) = store
        .aggregates(Query::Models {
            page: page(50, None),
        })
        .unwrap()
        .data
    else {
        panic!()
    };
    let unknown = groups
        .items
        .iter()
        .find(|g| g.attribution.id == "unknown:")
        .unwrap();
    assert_eq!(cost(&unknown.direct), (Some("100"), false));
    let response = Store::read_aggregates(&db, Query::Global).unwrap();
    let wire = serde_json::to_value(response).unwrap();
    assert_eq!(
        wire["data"]["data"]["estimatedCost"],
        serde_json::json!({"knownSubtotal":"100","complete":false})
    );
}

#[test]
fn aggregates_cost_exact_large_values_and_storage_errors() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("exact-cost.sqlite");
    let mut store = Store::open(&db).unwrap();
    add(&mut store, "a", None, 100, Some("alpha"), None);
    add(&mut store, "b", Some("a"), 30, Some("alpha"), None);
    settle(&mut store);
    price(&mut store, "alpha", "9223372036854.775807", 0);
    // 130 tokens times the maximum rate (i64::MAX micro-USD/million).
    assert_eq!(
        cost(&global(&mut store)),
        (Some("1199038364791120854910"), true)
    );
    assert_eq!(
        cost(session(&mut store, "a").inclusive.as_ref().unwrap()),
        (Some("1199038364791120854910"), true)
    );
    // Read-side corruption/range fixtures: these do not change the pricing writer.
    store
        .connection()
        .execute_batch(
            "DROP TRIGGER immutable_valuation_update; DROP TRIGGER immutable_valuation_delete;",
        )
        .unwrap();
    for bad in [
        "not-money",
        "1.5",
        "-1",
        "01",
        "170141183460469231731687303715884105728",
    ] {
        store.connection().execute("UPDATE observation_valuations SET amount=? WHERE observation_id=(SELECT MIN(observation_id) FROM observation_valuations)", [bad]).unwrap();
        assert!(
            matches!(
                Store::read_aggregates(&db, Query::Global),
                Err(ReadError::Storage)
            ),
            "{bad}"
        );
    }
    store
        .connection()
        .execute(
            "UPDATE observation_valuations SET amount=?",
            [i128::MAX.to_string()],
        )
        .unwrap();
    assert!(matches!(
        store.aggregates(Query::Global),
        Err(ReadError::Storage)
    ));
    // One boundary value remains exact; a missing valuation does not become zero.
    store.connection().execute("DELETE FROM observation_valuations WHERE observation_id=(SELECT MAX(observation_id) FROM observation_valuations)", []).unwrap();
    assert_eq!(
        cost(&global(&mut store)),
        (Some("170141183460469231731687303715884105727"), false)
    );
}

#[test]
fn aggregates_accept_contiguous_turn_reset_and_conserve_models_after_replay() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("turns.sqlite");
    // The second turn starts fresh at 30, while every thread category advances
    // by exactly that turn's explicit delta: 100 + 30 = 130 direct tokens.
    let first = serde_json::json!({"input_tokens":90,"cached_input_tokens":60,"cache_write_input_tokens":0,"output_tokens":10,"reasoning_output_tokens":4,"total_tokens":100});
    let second = serde_json::json!({"input_tokens":25,"cached_input_tokens":10,"cache_write_input_tokens":0,"output_tokens":5,"reasoning_output_tokens":2,"total_tokens":30});
    let endpoint = serde_json::json!({"input_tokens":115,"cached_input_tokens":70,"cache_write_input_tokens":0,"output_tokens":15,"reasoning_output_tokens":6,"total_tokens":130});
    let records = [
        serde_json::json!({"type":"session_meta","payload":{"id":"two-turns"}}),
        serde_json::json!({"type":"turn_context","payload":{"turn_id":"first","model":"alpha"}}),
        serde_json::json!({"timestamp":"2026-01-01T00:00:01Z","type":"token_usage_record","payload":{
            "thread_id":"two-turns","session_id":"two-turns","turn_id":"first","response_id":"first-response",
            "usage":first,"turn_token_usage":first,"thread_token_usage":first
        }}),
        serde_json::json!({"type":"turn_context","payload":{"turn_id":"second","model":"beta"}}),
        serde_json::json!({"timestamp":"2026-01-01T00:00:02Z","type":"token_usage_record","payload":{
            "thread_id":"two-turns","session_id":"two-turns","turn_id":"second","response_id":"second-response",
            "usage":second,"turn_token_usage":second,"thread_token_usage":endpoint
        }}),
    ];
    let mut store = Store::open(&db).unwrap();
    for record in &records {
        record_in_store(&mut store, "original", record);
    }
    add(
        &mut store,
        "child",
        Some("two-turns"),
        5,
        Some("gamma"),
        None,
    );
    settle(&mut store);
    assert_eq!(super::totals(&store), (135, 0, 3));
    assert_eq!(total(&session(&mut store, "two-turns").direct), Some("130"));
    drop(store);

    let mut store = Store::open(&db).unwrap();
    for record in &records {
        record_in_store(&mut store, "replay", record);
    }
    settle(&mut store);
    assert_eq!(super::totals(&store), (135, 0, 3));
    let parent = session(&mut store, "two-turns");
    assert_eq!(total(&parent.direct), Some("130"));
    assert_eq!(total(parent.inclusive.as_ref().unwrap()), Some("135"));
    assert!(!parent.direct.coverage.unresolved_usage);
    assert!(!parent.direct.coverage.unknown_model);
    assert_eq!(
        parent
            .direct
            .tokens
            .cached_input_tokens
            .known_tokens
            .as_deref(),
        Some("70")
    );
    assert_eq!(
        parent.direct.tokens.output_tokens.known_tokens.as_deref(),
        Some("15")
    );
    let Data::Models(models) = store
        .aggregates(Query::Models {
            page: page(50, None),
        })
        .unwrap()
        .data
    else {
        panic!()
    };
    let subtotals = models
        .items
        .iter()
        .map(|group| (group.attribution.id.as_str(), total(&group.direct)))
        .collect::<Vec<_>>();
    assert_eq!(
        subtotals,
        vec![
            ("model:alpha", Some("100")),
            ("model:beta", Some("30")),
            ("model:gamma", Some("5"))
        ]
    );
    assert_eq!(total(&models.direct), Some("135"));
    assert_eq!(total(&global(&mut store)), Some("135"));
}

#[test]
fn aggregates_conserve_direct_tree_project_model_and_paged_totals() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("aggregates.sqlite");
    let mut store = Store::open(&db).unwrap();
    let root = add(
        &mut store,
        "root",
        None,
        100,
        Some("alpha"),
        Some("C:/aggregate-a"),
    );
    add(
        &mut store,
        "child",
        Some("root"),
        30,
        Some("beta"),
        Some("C:/aggregate-b"),
    );
    add(&mut store, "grandchild", Some("child"), 5, None, None);
    record_in_store(&mut store, "duplicate", &root);
    settle(&mut store);
    assert_eq!(total(&global(&mut store)), Some("135"));
    for (id, direct, inclusive) in [
        ("root", "100", "135"),
        ("child", "30", "35"),
        ("grandchild", "5", "5"),
    ] {
        let value = session(&mut store, id);
        assert_eq!(total(&value.direct), Some(direct));
        assert_eq!(total(value.inclusive.as_ref().unwrap()), Some(inclusive));
    }
    let summary = global(&mut store);
    assert_eq!(
        summary.tokens.input_tokens.known_tokens.as_deref(),
        Some("132")
    );
    assert_eq!(
        summary.tokens.cached_input_tokens.known_tokens.as_deref(),
        Some("129")
    );
    assert_eq!(
        summary.tokens.reasoning_tokens.known_tokens.as_deref(),
        Some("3")
    );
    assert!(summary.coverage.unknown_model && summary.coverage.unattributed_project);
    assert_eq!(summary.observed_at.as_deref(), Some("2026-01-01T00:00:01Z"));
    for models in [false, true] {
        let mut after = None;
        let mut sums = Vec::new();
        loop {
            let query = if models {
                Query::Models {
                    page: page(1, after),
                }
            } else {
                Query::Projects {
                    page: page(1, after),
                }
            };
            let response = store.aggregates(query).unwrap();
            let groups = match response.data {
                Data::Projects(p) | Data::Models(p) => p,
                _ => panic!(),
            };
            assert_eq!(groups.total_items, 3);
            assert_eq!(groups.items.len(), 1);
            assert_eq!(total(&groups.direct), Some("135"));
            sums.push(
                total(&groups.items[0].direct)
                    .unwrap()
                    .parse::<i64>()
                    .unwrap(),
            );
            after = groups.next_cursor;
            if after.is_none() {
                break;
            }
        }
        sums.sort();
        assert_eq!(sums, vec![5, 30, 100]);
    }
    let Data::Sessions(first) = store
        .aggregates(Query::Sessions {
            page: page(1, None),
        })
        .unwrap()
        .data
    else {
        panic!()
    };
    assert_eq!(first.total_items, 3);
    assert_eq!(total(&first.direct), Some("135"));
    assert_eq!(first.items[0].thread_id, "child");
    let Data::Sessions(second) = store
        .aggregates(Query::Sessions {
            page: page(2, first.next_cursor),
        })
        .unwrap()
        .data
    else {
        panic!()
    };
    assert_eq!(
        second
            .items
            .iter()
            .map(|s| s.thread_id.as_str())
            .collect::<Vec<_>>(),
        vec!["grandchild", "root"]
    );
    assert_eq!(total(&second.direct), Some("135"));
    assert!(second.next_cursor.is_none());
    let Data::Sessions(ancestors) = store
        .aggregates(Query::Ancestors {
            thread: "grandchild".into(),
            page: page(1, None),
        })
        .unwrap()
        .data
    else {
        panic!()
    };
    assert_eq!(ancestors.total_items, 2);
    assert_eq!(total(&ancestors.direct), Some("130"));
    let Data::Sessions(children) = store
        .aggregates(Query::Children {
            thread: "root".into(),
            page: page(50, None),
        })
        .unwrap()
        .data
    else {
        panic!()
    };
    assert_eq!(children.total_items, 1);
    assert_eq!(total(&children.direct), Some("30"));
    let old_snapshot = serde_json::to_value(store.snapshot().unwrap()).unwrap();
    let response = store
        .aggregates(Query::Sessions {
            page: page(50, None),
        })
        .unwrap();
    let encoded = serde_json::to_string(&response).unwrap();
    for forbidden in [
        "FORBIDDEN_CONTENT",
        "normalized",
        "source_path",
        "response_id",
        "thread_token_usage",
    ] {
        assert!(!encoded.contains(forbidden));
    }
    assert_eq!(
        old_snapshot,
        serde_json::to_value(store.snapshot().unwrap()).unwrap()
    );
    drop(store);
    let Data::Global(reopened) = Store::read_aggregates(&db, Query::Global).unwrap().data else {
        panic!()
    };
    assert_eq!(total(&reopened), Some("135"));
}

#[test]
fn aggregates_gate_only_hierarchy_and_preserve_placeholders_cycles_and_long_paths() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("graph.sqlite")).unwrap();
    add(&mut store, "child", Some("missing"), 30, None, None);
    assert!(store.aggregates(Query::Global).unwrap().hierarchy_pending);
    let direct = session(&mut store, "child");
    assert_eq!(total(&direct.direct), Some("30"));
    assert!(direct.inclusive.is_none());
    assert_eq!(direct.parent_state, "pending");
    assert!(direct.parent_thread_id.is_none());
    assert!(matches!(
        store.aggregates(Query::Children {
            thread: "missing".into(),
            page: page(1, None)
        }),
        Err(ReadError::HierarchyPending)
    ));
    assert!(matches!(
        store.aggregates(Query::Ancestors {
            thread: "child".into(),
            page: page(1, None)
        }),
        Err(ReadError::HierarchyPending)
    ));
    assert!(store
        .aggregates(Query::Projects {
            page: page(1, None)
        })
        .is_ok());
    assert!(store
        .aggregates(Query::Models {
            page: page(1, None)
        })
        .is_ok());
    settle(&mut store);
    let placeholder = session(&mut store, "missing");
    assert!(placeholder.placeholder);
    assert_eq!(total(&placeholder.direct), None);
    assert_eq!(placeholder.direct.observed_sessions, 0);
    assert_eq!(placeholder.project.basis, "unavailable");
    assert_eq!(total(placeholder.inclusive.as_ref().unwrap()), Some("30"));
    add(&mut store, "a", Some("b"), 10, None, None);
    add(&mut store, "b", Some("a"), 20, None, None);
    add(&mut store, "descendant", Some("a"), 5, None, None);
    settle(&mut store);
    assert_eq!(
        total(session(&mut store, "a").inclusive.as_ref().unwrap()),
        Some("15")
    );
    assert_eq!(
        total(session(&mut store, "b").inclusive.as_ref().unwrap()),
        Some("20")
    );
    assert_eq!(total(&global(&mut store)), Some("65"));
    // More than one page and more than the resolver's per-step work budget.
    for n in 0..70 {
        add(
            &mut store,
            &format!("chain-{n:03}"),
            (n > 0).then(|| format!("chain-{:03}", n - 1)).as_deref(),
            2,
            None,
            None,
        );
    }
    settle(&mut store);
    assert_eq!(
        total(session(&mut store, "chain-000").inclusive.as_ref().unwrap()),
        Some("140")
    );
    let Data::Sessions(ancestors) = store
        .aggregates(Query::Ancestors {
            thread: "chain-069".into(),
            page: page(50, None),
        })
        .unwrap()
        .data
    else {
        panic!()
    };
    assert_eq!(ancestors.total_items, 69);
    assert_eq!(ancestors.items.len(), 50);
    assert_eq!(total(&ancestors.direct), Some("138"));
    assert!(ancestors.next_cursor.is_some());
}

#[test]
fn aggregates_unavailable_categories_legacy_and_rejected_usage_are_honest() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::open(&temp.path().join("coverage.sqlite")).unwrap();
    add(&mut store, "known", None, 10, Some("alpha"), None);
    add(&mut store, "unknown", None, 20, None, None);
    add(&mut store, "rejected", None, 40, None, None);
    settle(&mut store);
    // Durable projections from older adapters may lack a category; no new source
    // acceptance is implied by this read-side compatibility fixture.
    store.connection().execute("UPDATE observations SET normalized=json_remove(normalized,'$.usage.cached_input_tokens') WHERE thread_id='unknown'",[]).unwrap();
    store.connection().execute("UPDATE observations SET normalized=json_set(normalized,'$.usage.reasoning_output_tokens',NULL) WHERE thread_id='unknown'",[]).unwrap();
    store.connection().execute("UPDATE observations SET accepted=0,total=NULL,state='rejected' WHERE thread_id='rejected'",[]).unwrap();
    record_in_store(
        &mut store,
        "legacy",
        &serde_json::json!({"type":"session_meta","payload":{"id":"legacy"}}),
    );
    record_in_store(
        &mut store,
        "legacy",
        &serde_json::json!({"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"total_tokens":999}}}}),
    );
    settle(&mut store);
    let summary = global(&mut store);
    assert_eq!(total(&summary), Some("30"));
    assert_eq!(
        summary.tokens.cached_input_tokens.known_tokens.as_deref(),
        Some("8")
    );
    assert!(!summary.tokens.cached_input_tokens.complete);
    assert_eq!(
        summary.tokens.reasoning_tokens.known_tokens.as_deref(),
        Some("1")
    );
    assert!(!summary.tokens.reasoning_tokens.complete);
    assert!(summary.tokens.total_tokens.complete);
    assert!(summary.coverage.unresolved_usage && summary.coverage.source_diagnostics);
    assert_eq!(summary.coverage.unavailable_sessions, 2);
    assert_eq!(total(&session(&mut store, "legacy").direct), None);
    record_in_store(
        &mut store,
        "known",
        &serde_json::json!({"type":"turn_context","payload":{"turn_id":"known","model":"conflicting"}}),
    );
    let Data::Models(models) = store
        .aggregates(Query::Models {
            page: page(50, None),
        })
        .unwrap()
        .data
    else {
        panic!()
    };
    assert_eq!(models.total_items, 1);
    assert_eq!(models.items[0].attribution.id, "unknown:");
    assert_eq!(total(&models.items[0].direct), Some("30"));
    assert_eq!(models.items[0].direct.coverage.unavailable_sessions, 2);
    for limit in [0, 51, u32::MAX] {
        assert!(matches!(
            store.aggregates(Query::Sessions {
                page: page(limit, None)
            }),
            Err(ReadError::InvalidQuery)
        ));
    }
}

#[test]
fn aggregates_merge_only_confirmed_worktrees_and_migrate_read_indexes() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repository");
    let linked = temp.path().join("linked");
    std::fs::create_dir_all(root.join(".git/worktrees/linked")).unwrap();
    std::fs::create_dir_all(&linked).unwrap();
    std::fs::write(
        linked.join(".git"),
        format!("gitdir: {}", root.join(".git/worktrees/linked").display()),
    )
    .unwrap();
    std::fs::write(root.join(".git/worktrees/linked/commondir"), "../..").unwrap();
    let db = temp.path().join("migration.sqlite");
    let mut store = Store::open(&db).unwrap();
    add(&mut store, "root", None, 100, Some("alpha"), root.to_str());
    add(
        &mut store,
        "linked",
        None,
        30,
        Some("alpha"),
        linked.to_str(),
    );
    add(
        &mut store,
        "location",
        None,
        5,
        None,
        Some("C:/unresolved-repository"),
    );
    settle(&mut store);
    let Data::Projects(groups) = store
        .aggregates(Query::Projects {
            page: page(50, None),
        })
        .unwrap()
        .data
    else {
        panic!()
    };
    assert_eq!(groups.total_items, 2);
    assert_eq!(groups.items[0].attribution.basis, "locationDerived");
    assert_eq!(total(&groups.items[0].direct), Some("5"));
    assert_eq!(groups.items[1].attribution.basis, "confirmedRepository");
    assert_eq!(total(&groups.items[1].direct), Some("130"));
    // Restore a v5-shaped fixture, including removal of later pricing schema.
    store.connection().execute_batch("DROP TABLE pricing_work; DROP TABLE observation_valuations; DROP TABLE model_price_versions; DROP TABLE detected_models; DROP INDEX observation_pricing_model; DROP INDEX session_effective_children; DROP INDEX observation_model_bucket; PRAGMA user_version=5;").unwrap();
    drop(store);
    let mut store = Store::open(&db).unwrap();
    assert_eq!(total(&global(&mut store)), Some("135"));
    for (sql,index) in [("SELECT thread_id FROM sessions WHERE parent_thread_id='root' AND parent_state='available'","session_effective_children"),("SELECT DISTINCT CASE WHEN model IS NULL THEN 'unknown:' ELSE 'model:' || model END FROM observations","observation_model_bucket")] {
        let plan=store.connection().prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap().query_map([],|r|r.get::<_,String>(3)).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap();
        assert!(plan.iter().any(|s|s.contains(index)),"{plan:?}");
    }
}
