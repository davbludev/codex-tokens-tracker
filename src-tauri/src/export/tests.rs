use super::*;
use crate::{
    adapter,
    pricing::{CacheWritePolicy, PriceInput, ReasoningPolicy},
    storage::Store,
};
use serde_json::json;

fn record(store: &mut Store, source: &str, value: serde_json::Value) {
    let (offset, ordinal) = store.checkpoint(source).unwrap();
    let encoded = format!("{value}\n");
    store
        .line(
            source,
            offset,
            offset + encoded.len() as u64,
            ordinal + 1,
            adapter::decode(encoded.as_bytes()),
        )
        .unwrap();
}

fn usage(store: &mut Store, id: &str, model: &str, project: &str, timestamp: &str, total: u64) {
    record(
        store,
        id,
        json!({"type":"session_meta","payload":{"id":id,"cwd":project,"prompt":"DO_NOT_EXPORT_PROMPT"}}),
    );
    record(
        store,
        id,
        json!({"type":"turn_context","payload":{"turn_id":"turn","model":model}}),
    );
    let tokens = json!({"input_tokens":total,"cached_input_tokens":0,"cache_write_input_tokens":0,"output_tokens":0,"reasoning_output_tokens":0,"total_tokens":total});
    record(
        store,
        id,
        json!({"type":"token_usage_record","timestamp":timestamp,"payload":{"thread_id":id,"turn_id":"turn","response_id":"response","usage":tokens,"thread_token_usage":tokens,"message":"DO_NOT_EXPORT_MESSAGE"}}),
    );
}

fn price(store: &mut Store, model: &str, rate: &str) {
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
            true,
            (0, 0),
        )
        .unwrap();
    while store.pricing_work_pending().unwrap() {
        store.process_pricing_work().unwrap();
    }
}

fn settle(store: &mut Store) {
    for _ in 0..1000 {
        if !store.reconcile_pending().unwrap() {
            return;
        }
    }
    panic!("hierarchy did not settle");
}

fn report(db: &Path, kind: Kind, destination: &Path) -> (Outcome, String) {
    let result = export_at(
        db,
        Request {
            kind,
            destination: destination.to_string_lossy().into_owned(),
        },
        Time {
            seconds: 2_000_000_000,
            nanos: 0,
        },
    )
    .unwrap();
    let contents = fs::read_to_string(destination).unwrap();
    assert!(!contents.contains("DO_NOT_EXPORT"));
    (result, contents)
}

// Numeric scenarios have no embedded delimiters; header lookup avoids coupling
// assertions to column positions while the escaping scenario checks raw RFC rows.
fn field<'a>(contents: &'a str, row: &str, header: &str) -> &'a str {
    let index = contents
        .lines()
        .next()
        .unwrap()
        .split(',')
        .position(|value| value == header)
        .unwrap();
    contents
        .lines()
        .skip(1)
        .find(|value| value.starts_with(&format!("{row},")))
        .unwrap()
        .split(',')
        .nth(index)
        .unwrap()
}

#[test]
fn usage_exports_conserve_direct_scopes_and_exact_money_with_unpriced_states() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("usage.sqlite");
    let mut store = Store::open(&db).unwrap();
    usage(
        &mut store,
        "root",
        "priced",
        "C:/shared",
        "2026-01-01T00:01:00Z",
        3,
    );
    usage(
        &mut store,
        "child",
        "priced",
        "C:/shared",
        "2026-01-01T00:02:00Z",
        2,
    );
    record(
        &mut store,
        "child",
        json!({"type":"session_meta","payload":{"id":"child","parent_thread_id":"root","cwd":"C:/shared"}}),
    );
    usage(
        &mut store,
        "unknown",
        "unpriced",
        "C:/shared",
        "2026-01-01T00:03:00Z",
        7,
    );
    record(
        &mut store,
        "empty",
        json!({"type":"session_meta","payload":{"id":"empty","cwd":"C:/empty"}}),
    );
    settle(&mut store);
    price(&mut store, "priced", "1801439850.948199");
    // 5 tokens * rate / 1,000,000 = 9007.199254740995 USD,
    // whose trillionths exceed f64's exact integer range.
    let (sessions, text) = report(&db, Kind::Sessions, &temp.path().join("sessions.csv"));
    assert_eq!(sessions.row_count, 4);
    assert_eq!(field(&text, "root", "total_tokens_known_subtotal"), "3");
    assert_eq!(field(&text, "child", "total_tokens_known_subtotal"), "2");
    assert_eq!(field(&text, "unknown", "estimated_cost_state"), "unpriced");
    assert_eq!(
        field(&text, "unknown", "estimated_cost_known_subtotal_usd"),
        ""
    );
    assert_eq!(field(&text, "empty", "usage_state"), "unavailable");
    assert_eq!(field(&text, "root", "usage_scope"), "direct");

    let (models, text) = report(&db, Kind::ModelUsage, &temp.path().join("models.csv"));
    assert_eq!(models.row_count, 3);
    assert_eq!(
        field(&text, "model:priced", "total_tokens_known_subtotal"),
        "5"
    );
    assert_eq!(
        field(&text, "model:priced", "estimated_cost_known_subtotal_usd"),
        "9007.199254740995"
    );
    assert_eq!(
        field(&text, "model:unpriced", "estimated_cost_state"),
        "unpriced"
    );
    assert_eq!(
        field(&text, "unknown:", "total_tokens_state"),
        "unavailable"
    );

    let (projects, text) = report(&db, Kind::ProjectTotals, &temp.path().join("projects.csv"));
    assert_eq!(projects.row_count, 2);
    let project = text
        .lines()
        .skip(1)
        .find(|row| row.contains("9007.199254740995"))
        .unwrap()
        .split(',')
        .next()
        .unwrap();
    assert_eq!(field(&text, project, "total_tokens_known_subtotal"), "12");
    assert_eq!(
        field(&text, project, "estimated_cost_known_subtotal_usd"),
        "9007.199254740995"
    );
    assert_eq!(
        field(&text, project, "estimated_cost_state"),
        "incomplete_unpriced_usage"
    );
}

#[test]
fn csv_preserves_quotes_commas_crlf_and_unicode_metadata_without_raw_content() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("quoted.sqlite");
    let mut store = Store::open(&db).unwrap();
    usage(
        &mut store,
        "root",
        "model,\"雪\"\r\nnext",
        "C:/quoted",
        "2026-01-01T00:01:00Z",
        1,
    );
    settle(&mut store);
    let (_, text) = report(&db, Kind::ModelUsage, &temp.path().join("quoted.csv"));
    assert!(
        text.contains(
            "\"model:model,\"\"雪\"\"\r\nnext\",observedModel,\"model,\"\"雪\"\"\r\nnext\""
        ),
        "{text}"
    );
    assert!(text.ends_with("\r\n"));
}

fn limit(store: &mut Store, timestamp: &str, used: u32) {
    record(
        store,
        "limits",
        json!({"type":"event_msg","timestamp":timestamp,"payload":{"type":"token_count","rate_limits":{"limit_id":"codex","secondary":{"used_percent":used,"window_minutes":10080,"resets_at":2_000_000_000_i64}}}}),
    );
}

#[test]
fn weekly_export_uses_comparable_intervals_and_retains_all_cycles() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("weekly.sqlite");
    let mut store = Store::open(&db).unwrap();
    limit(&mut store, "2026-01-01T00:00:00Z", 40);
    limit(&mut store, "2026-01-01T00:10:00Z", 42);
    limit(&mut store, "2026-01-01T00:20:00Z", 1);
    limit(&mut store, "2026-01-01T00:30:00Z", 3);
    limit(&mut store, "2026-01-01T00:40:00Z", 0);
    usage(
        &mut store,
        "priced",
        "priced",
        "C:/p",
        "2026-01-01T00:05:00Z",
        1_000_000,
    );
    usage(
        &mut store,
        "unpriced",
        "unpriced",
        "C:/p",
        "2026-01-01T00:25:00Z",
        7,
    );
    usage(
        &mut store,
        "unmatched",
        "priced",
        "C:/p",
        "2026-01-01T00:11:00Z",
        1_000_000,
    );
    price(&mut store, "priced", "1");
    let (result, text) = report(&db, Kind::WeeklyCycles, &temp.path().join("weekly.csv"));
    assert_eq!(result.row_count, 3);
    let first = format!(
        "{}:000000000",
        adapter::observation_time("2026-01-01T00:00:00Z").unwrap().0
    );
    let second = format!(
        "{}:000000000",
        adapter::observation_time("2026-01-01T00:20:00Z").unwrap().0
    );
    let current = format!(
        "{}:000000000",
        adapter::observation_time("2026-01-01T00:40:00Z").unwrap().0
    );
    assert_eq!(field(&text, &first, "cycle_state"), "completed");
    assert_eq!(
        field(&text, &first, "total_tokens_known_subtotal"),
        "1000000"
    );
    assert_eq!(
        field(&text, &first, "estimated_cost_known_subtotal_usd"),
        "1.000000000000"
    );
    assert_eq!(
        field(&text, &first, "observed_usd_per_percentage_point"),
        "0.500000000000"
    );
    assert_eq!(field(&text, &first, "full_cycle_cost_state"), "unavailable");
    assert_eq!(field(&text, &second, "estimate_state"), "unpriced_usage");
    assert_eq!(
        field(&text, &second, "observed_usd_per_percentage_point"),
        ""
    );
    assert_eq!(field(&text, &current, "cycle_state"), "current");
    assert_eq!(
        field(&text, &current, "estimate_state"),
        "insufficient_observations"
    );
}

#[test]
fn exports_reject_existing_files_invalid_paths_and_remove_failed_outputs() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("database.csv");
    let store = Store::open(&db).unwrap();
    let run = |destination: &Path| {
        export_csv(
            &db,
            Request {
                kind: Kind::Sessions,
                destination: destination.to_string_lossy().into_owned(),
            },
        )
    };
    assert_eq!(run(&db).unwrap_err(), Error::DestinationExists);
    let source = temp.path().join("source.csv");
    fs::write(&source, "source metadata must survive").unwrap();
    assert_eq!(run(&source).unwrap_err(), Error::DestinationExists);
    assert_eq!(
        fs::read_to_string(&source).unwrap(),
        "source metadata must survive"
    );
    assert_eq!(
        run(Path::new("relative.csv")).unwrap_err(),
        Error::InvalidDestination
    );
    assert_eq!(
        run(&temp.path().join("source.jsonl")).unwrap_err(),
        Error::InvalidDestination
    );
    assert_eq!(
        run(&temp.path().join("missing/output.csv")).unwrap_err(),
        Error::Write
    );
    let destination = temp.path().join("broken.csv");
    store
        .connection()
        .execute_batch("DROP TABLE observation_valuations")
        .unwrap();
    assert_eq!(run(&destination).unwrap_err(), Error::Storage);
    assert!(!destination.exists());
    assert!(store
        .connection()
        .query_row("SELECT COUNT(*) FROM sessions", [], |row| row
            .get::<_, i64>(0))
        .is_ok());
}

#[test]
fn streaming_export_stays_in_one_snapshot_during_live_ingestion() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("snapshot.sqlite");
    let mut writer = Store::open(&db).unwrap();
    for index in 0..125 {
        usage(
            &mut writer,
            &format!("session-{index:03}"),
            "model",
            "C:/p",
            "2026-01-01T00:00:00Z",
            1,
        );
    }
    let mut reader = Connection::open_with_flags(&db, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    weekly_storage::register(&reader).unwrap();
    let snapshot = reader.transaction().unwrap();
    snapshot
        .query_row("SELECT COUNT(*) FROM sessions", [], |_| Ok(()))
        .unwrap();
    usage(
        &mut writer,
        "new-live-session",
        "model",
        "C:/p",
        "2026-01-01T00:01:00Z",
        99,
    );
    let mut output = Vec::new();
    let count = write_report(
        &snapshot,
        Kind::Sessions,
        Time {
            seconds: 2_000_000_000,
            nanos: 0,
        },
        &mut output,
    )
    .unwrap();
    assert_eq!(
        count, 125,
        "all rows beyond a UI page are exported from the fixed snapshot"
    );
    assert!(!String::from_utf8(output)
        .unwrap()
        .contains("new-live-session"));

    struct FullDevice {
        written: usize,
    }
    impl Write for FullDevice {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            let accepted = bytes.len().min(2048 - self.written);
            if accepted == 0 {
                return Err(std::io::Error::other("device full"));
            }
            self.written += accepted;
            Ok(accepted)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut full_device = FullDevice { written: 0 };
    assert_eq!(
        write_report(
            &snapshot,
            Kind::Sessions,
            Time {
                seconds: 2_000_000_000,
                nanos: 0
            },
            &mut full_device
        )
        .unwrap_err(),
        Error::Write
    );
    assert_eq!(
        full_device.written, 2048,
        "a failure after partial output is surfaced"
    );
}
