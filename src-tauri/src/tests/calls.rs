use crate::{
    activity, adapter, calls,
    pricing::{CacheWritePolicy, PriceInput, ReasoningPolicy},
    source,
    storage::Store,
    weekly::Time,
};
use serde_json::{json, Value};
use std::{fs, path::Path};

fn time(seconds: i64) -> Time {
    Time { seconds, nanos: 0 }
}
fn usage(n: i64, timestamp: i64) -> Value {
    json!({"type":"token_usage_record","timestamp":stamp(timestamp),"payload":{"thread_id":"task","session_id":"task","turn_id":"turn","response_id":format!("response-{n}"),"usage":{"input_tokens":100,"cached_input_tokens":80,"cache_write_input_tokens":0,"output_tokens":20,"reasoning_output_tokens":5,"total_tokens":120},"thread_token_usage":{"input_tokens":100*n,"cached_input_tokens":80*n,"cache_write_input_tokens":0,"output_tokens":20*n,"reasoning_output_tokens":5*n,"total_tokens":120*n}}})
}
fn stamp(seconds: i64) -> String {
    format!("2026-01-01T00:{:02}:{:02}Z", seconds / 60, seconds % 60)
}
fn base() -> i64 {
    adapter::observation_time(&stamp(0)).unwrap().0
}
fn window(start: i64, end: i64) -> calls::Window {
    calls::Window {
        start: time(base() + start),
        end: time(base() + end),
    }
}
fn query(start: i64, end: i64) -> calls::Query {
    let window = window(start, end);
    calls::Query {
        start: window.start,
        end: window.end,
        model: None,
        thread: None,
        after: None,
        limit: None,
    }
}
fn event(seconds: i64, kind: &str, payload: Value) -> Value {
    json!({"timestamp":stamp(seconds),"type":kind,"payload":payload})
}
fn setup(path: &Path, lines: &[Value]) -> (Store, std::path::PathBuf, std::path::PathBuf) {
    let db = path.join("usage.sqlite");
    let log = path.join("rollout-test.jsonl");
    let mut content = json!({"type":"session_meta","payload":{"id":"task"}}).to_string() + "\n";
    content += &(event(
        0,
        "turn_context",
        json!({"turn_id":"turn","model":"model","effort":"high"}),
    )
    .to_string()
        + "\n");
    for line in lines {
        content += &(line.to_string() + "\n");
    }
    fs::write(&log, content).unwrap();
    let mut store = Store::open(&db).unwrap();
    source::ingest(&mut store, &log).unwrap();
    store
        .save_model_price_at("model", price("2"), true, (0, 0))
        .unwrap();
    while store.pricing_work_pending().unwrap() {
        store.process_pricing_work().unwrap();
    }
    (store, db, log)
}
fn price(input: &str) -> PriceInput {
    PriceInput {
        input: input.into(),
        cached_input: "0.2".into(),
        cache_write: "3".into(),
        output: "10".into(),
        reasoning: None,
        reasoning_policy: ReasoningPolicy::Included,
        cache_write_policy: CacheWritePolicy::Additional,
    }
}
fn activity_query(id: &str, start: i64, end: i64) -> activity::Query {
    let window = window(start, end);
    activity::Query {
        observation_id: id.into(),
        start: window.start,
        end: window.end,
        cursor: None,
    }
}
fn all_activity(
    runtime: &activity::Runtime,
    db: &Path,
    mut query: activity::Query,
) -> Vec<activity::Event> {
    let mut events = Vec::new();
    for _ in 0..100 {
        let page = runtime.activity(db, query.clone()).unwrap();
        events.extend(page.events);
        if page.next_cursor.is_none() {
            return events;
        }
        query.cursor = page.next_cursor;
    }
    panic!("Activity cursor did not advance");
}

#[test]
fn calls_are_separate_with_exact_bounds_paging_and_preserved_category_prices() {
    let temp = tempfile::tempdir().unwrap();
    let lines: Vec<_> = (1..=55).map(|n| usage(n, n)).collect();
    let (mut store, _, log) = setup(temp.path(), &lines);
    let first = store.calls(query(0, 55)).unwrap();
    assert_eq!(first.total_items, 55);
    assert_eq!(first.items.len(), 50);
    assert_eq!(
        first.summary.estimated_cost.known_subtotal.as_deref(),
        Some("14080000000")
    );
    let call = &first.items[0];
    assert_eq!(call.turn_id.as_deref(), Some("turn"));
    assert_eq!(
        call.categories.input.tokens.known_tokens.as_deref(),
        Some("20")
    );
    assert_eq!(
        call.categories.cached_input.tokens.known_tokens.as_deref(),
        Some("80")
    );
    assert_eq!(
        call.categories.output.tokens.known_tokens.as_deref(),
        Some("20")
    );
    assert_eq!(
        call.estimated_cost.known_subtotal.as_deref(),
        Some("256000000")
    );
    assert_eq!(
        call.categories
            .output
            .estimated_cost
            .known_subtotal
            .as_deref(),
        Some("200000000")
    );
    let mut second_query = query(0, 55);
    second_query.after = first.next_cursor;
    let second = store.calls(second_query).unwrap();
    assert_eq!(second.items.len(), 5);
    assert!(second.next_cursor.is_none());
    assert_eq!(
        second.summary.estimated_cost.known_subtotal,
        first.summary.estimated_cost.known_subtotal
    );
    let middle = store.calls(query(20, 21)).unwrap();
    assert_eq!(middle.total_items, 1);
    assert_eq!(middle.items[0].response_id.as_deref(), Some("response-21"));
    // Same session continues well beyond this selection; no lifetime totals leak.
    assert_eq!(
        middle.summary.estimated_cost.known_subtotal.as_deref(),
        Some("256000000")
    );
    store
        .save_model_price_at("model", price("99"), false, (base() + 60, 0))
        .unwrap();
    assert_eq!(
        store
            .calls(query(20, 21))
            .unwrap()
            .summary
            .estimated_cost
            .known_subtotal,
        middle.summary.estimated_cost.known_subtotal
    );
    source::ingest(&mut store, &log).unwrap();
    assert_eq!(store.calls(query(0, 55)).unwrap().total_items, 55);
    let mut wrong = query(0, 55);
    wrong.model = Some("other".into());
    assert_eq!(store.calls(wrong).unwrap().total_items, 0);
    let mut invalid = query(21, 20);
    assert!(store.calls(invalid).is_err());
    invalid = query(0, 55);
    invalid.after = Some("bad cursor".into());
    assert!(store.calls(invalid).is_err());
}

#[test]
fn activity_groups_a_tool_batch_and_never_reveals_out_of_range_results() {
    let temp = tempfile::tempdir().unwrap();
    let lines = vec![
        event(
            1,
            "response_item",
            json!({"type":"custom_tool_call","call_id":"batch","name":"functions.exec","input":"run two commands"}),
        ),
        usage(1, 2),
        event(
            3,
            "event_msg",
            json!({"type":"item_completed","turn_id":"turn","item":{"type":"CommandExecution","id":"command-1","command":"rtk read src/main.rs","cwd":"D:/project","output":"read output","status":"completed"}}),
        ),
        event(
            4,
            "event_msg",
            json!({"type":"item_completed","turn_id":"turn","item":{"type":"FileChange","id":"command-2","changes":{"src/main.rs":{"update":{"unified_diff":"+ change"}}},"status":"completed"}}),
        ),
        event(
            5,
            "response_item",
            json!({"type":"custom_tool_call_output","call_id":"batch","output":"complete result"}),
        ),
        event(
            8,
            "response_item",
            json!({"type":"message","id":"message-2","role":"assistant","content":[{"type":"output_text","text":"later response"}]}),
        ),
        usage(2, 9),
    ];
    let (mut store, db, _) = setup(temp.path(), &lines);
    let calls = store.calls(query(0, 10)).unwrap();
    let runtime = activity::Runtime::default();
    let events = all_activity(&runtime, &db, activity_query(&calls.items[0].id, 0, 6));
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == "custom_tool_call")
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.association == "activeBatch")
            .count(),
        2
    );
    assert!(events
        .iter()
        .any(|event| event.paths == ["src/main.rs"] && event.paths_inferred));
    assert!(events
        .iter()
        .all(|event| event.label != "Assistant message"));
    let result = events
        .iter()
        .find(|event| event.kind == "toolOutput")
        .unwrap();
    assert_eq!(
        runtime
            .text(activity::TextQuery {
                text_ref: result.fields[0].text_ref.clone(),
                offset: None
            })
            .unwrap()
            .text,
        "complete result"
    );
    let clipped = all_activity(&runtime, &db, activity_query(&calls.items[0].id, 0, 4));
    assert!(clipped.iter().all(|event| event.time <= time(base() + 4)));
    assert!(!clipped.iter().any(|event| event.kind == "toolOutput"));
    assert_eq!(
        store
            .calls(query(0, 4))
            .unwrap()
            .summary
            .estimated_cost
            .known_subtotal
            .as_deref(),
        Some("256000000")
    );
    let second = all_activity(&runtime, &db, activity_query(&calls.items[1].id, 0, 10));
    assert!(
        second
            .iter()
            .any(|event| event.label == "Assistant message" && event.association == "logOrder"),
        "A prior invocation's output must not make this generation ambiguous"
    );
}

#[test]
fn activity_text_is_paged_and_changed_or_missing_sources_leave_costs_intact() {
    let temp = tempfile::tempdir().unwrap();
    let text = "текст 🦀\n".repeat(16000);
    let (mut store, db, log) = setup(
        temp.path(),
        &[
            event(
                1,
                "response_item",
                json!({"type":"message","role":"assistant","id":"msg","content":text}),
            ),
            usage(1, 2),
        ],
    );
    let page = store.calls(query(0, 3)).unwrap();
    let id = &page.items[0].id;
    let runtime = activity::Runtime::default();
    let events = all_activity(&runtime, &db, activity_query(id, 0, 3));
    let field = &events[0].fields[0];
    let mut offset = None;
    let mut restored = String::new();
    loop {
        let page = runtime
            .text(activity::TextQuery {
                text_ref: field.text_ref.clone(),
                offset,
            })
            .unwrap();
        assert!(page.text.len() <= 65536);
        restored += &page.text;
        offset = page.next_offset;
        if offset.is_none() {
            break;
        }
    }
    assert_eq!(restored, text);
    let bytes = fs::read(&log).unwrap();
    fs::write(&log, b"replaced").unwrap();
    assert!(runtime
        .text(activity::TextQuery {
            text_ref: field.text_ref.clone(),
            offset: None
        })
        .is_err());
    let failed = runtime.activity(&db, activity_query(id, 0, 3)).unwrap();
    assert_eq!(failed.association, "unavailable");
    fs::write(&log, bytes).unwrap();
    fs::remove_file(&log).unwrap();
    assert_eq!(
        runtime
            .activity(&db, activity_query(id, 0, 3))
            .unwrap()
            .association,
        "unavailable"
    );
    assert_eq!(
        store
            .calls(query(0, 3))
            .unwrap()
            .summary
            .estimated_cost
            .known_subtotal,
        page.summary.estimated_cost.known_subtotal
    );
}

#[test]
fn ambiguous_generations_and_compaction_never_create_additional_charges() {
    let temp = tempfile::tempdir().unwrap();
    let lines = vec![
        event(
            1,
            "response_item",
            json!({"type":"function_call","call_id":"a","name":"read","arguments":"x"}),
        ),
        event(
            2,
            "response_item",
            json!({"type":"function_call_output","call_id":"a","output":"x"}),
        ),
        event(
            3,
            "response_item",
            json!({"type":"message","role":"assistant","id":"msg","content":"another generation"}),
        ),
        usage(1, 4),
        event(
            5,
            "compacted",
            json!({"compaction_response_id":"response-2","message":"summary","latest_token_usage_record":usage(2,6)}),
        ),
        usage(2, 6),
    ];
    let (mut store, db, _) = setup(temp.path(), &lines);
    let calls = store.calls(query(0, 7)).unwrap();
    assert_eq!(calls.total_items, 2);
    let runtime = activity::Runtime::default();
    let events = all_activity(&runtime, &db, activity_query(&calls.items[0].id, 0, 7));
    assert!(events.iter().any(|event| event.association == "ambiguous"));
    let events = all_activity(&runtime, &db, activity_query(&calls.items[1].id, 0, 7));
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == "compaction" && event.association == "responseId")
            .count(),
        1
    );
    assert_eq!(store.calls(query(0, 7)).unwrap().total_items, 2);
}

#[test]
fn billed_quantities_follow_cache_write_policy_and_partial_prices_stay_partial() {
    let temp = tempfile::tempdir().unwrap();
    let mut value = usage(1, 2);
    value["payload"]["usage"]["cache_write_input_tokens"] = 10.into();
    value["payload"]["thread_token_usage"]["cache_write_input_tokens"] = 10.into();
    let db = temp.path().join("categories.sqlite");
    let log = temp.path().join("rollout-categories.jsonl");
    let mut store = Store::open(&db).unwrap();
    let mut rates = price("2");
    rates.cache_write_policy = CacheWritePolicy::IncludedInputDisjoint;
    rates.reasoning_policy = ReasoningPolicy::Separate;
    rates.reasoning = Some("4".into());
    fs::write(
        &log,
        format!(
            "{}\n{}\n{}\n",
            json!({"type":"session_meta","payload":{"id":"task"}}),
            event(0, "turn_context", json!({"turn_id":"turn","model":"model"})),
            value
        ),
    )
    .unwrap();
    source::ingest(&mut store, &log).unwrap();
    store
        .save_model_price_at("model", rates, true, (0, 0))
        .unwrap();
    while store.pricing_work_pending().unwrap() {
        store.process_pricing_work().unwrap();
    }
    let priced = store.calls(query(0, 3)).unwrap();
    let call = &priced.items[0];
    assert_eq!(
        call.categories.input.tokens.known_tokens.as_deref(),
        Some("10")
    );
    assert_eq!(
        call.categories.output.tokens.known_tokens.as_deref(),
        Some("20")
    );
    assert_eq!(
        call.estimated_cost.known_subtotal.as_deref(),
        Some("236000000")
    );
    assert_eq!(
        call.categories
            .output
            .estimated_cost
            .known_subtotal
            .as_deref(),
        Some("170000000")
    );
    let other = temp.path().join("rollout-unpriced.jsonl");
    let mut unknown = usage(1, 2);
    unknown["payload"]["thread_id"] = "unknown".into();
    unknown["payload"]["session_id"] = "unknown".into();
    fs::write(
        &other,
        format!(
            "{}\n{}\n",
            json!({"type":"session_meta","payload":{"id":"unknown"}}),
            unknown
        ),
    )
    .unwrap();
    source::ingest(&mut store, &other).unwrap();
    let mixed = store.calls(query(0, 3)).unwrap();
    assert_eq!(mixed.total_items, 2);
    assert!(!mixed.summary.estimated_cost.complete);
    assert_eq!(
        mixed.summary.estimated_cost.known_subtotal.as_deref(),
        Some("236000000")
    );
    assert!(
        !mixed
            .summary
            .categories
            .cached_input
            .estimated_cost
            .complete
    );
    assert_eq!(
        mixed
            .summary
            .categories
            .cached_input
            .tokens
            .known_tokens
            .as_deref(),
        Some("160")
    );
    assert!(mixed
        .items
        .iter()
        .any(|call| call.model.is_none() && call.estimated_cost.known_subtotal.is_none()));
}

#[test]
fn activity_scan_continues_across_large_sources_and_resolves_a_registered_archive() {
    let temp = tempfile::tempdir().unwrap();
    let mut lines: Vec<_> = (0..70)
        .map(|_| {
            event(
                0,
                "event_msg",
                json!({"type":"background_context","text":"x".repeat(70_000)}),
            )
        })
        .collect();
    lines.extend([event(1,"response_item",json!({"type":"message","role":"assistant","id":"message","content":"hello after a large prefix"})),usage(1,2)]);
    let (mut store, db, log) = setup(temp.path(), &lines);
    let calls = store.calls(query(0, 3)).unwrap();
    let id = &calls.items[0].id;
    let runtime = activity::Runtime::default();
    let first = runtime.activity(&db, activity_query(id, 0, 3)).unwrap();
    assert!(first.events.is_empty());
    assert!(first.next_cursor.is_some());
    assert_eq!(first.scanned_bytes, 4 * 1024 * 1024);
    let mut continuation = activity_query(id, 0, 3);
    continuation.cursor = first.next_cursor;
    let events = all_activity(&runtime, &db, continuation);
    assert!(events
        .iter()
        .any(|event| event.label == "Assistant message"));
    let archived = temp.path().join("rollout-archived.jsonl");
    fs::rename(&log, &archived).unwrap();
    source::ingest(&mut store, &archived).unwrap();
    assert_eq!(store.calls(query(0, 3)).unwrap().total_items, 1);
    let events = all_activity(&runtime, &db, activity_query(id, 0, 3));
    assert!(
        events
            .iter()
            .any(|event| event.label == "Assistant message"),
        "Registered archive copies retain access to the same verified usage anchor"
    );
}
