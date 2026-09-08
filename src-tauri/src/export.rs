//! CSV delivery streams prepared metadata from one SQLite snapshot to a new file.
use crate::{
    aggregates::{Category, Tokens},
    storage::{aggregates, weekly as weekly_storage},
    weekly::{self, CompletedCycle, Time, Timeline},
};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{BufWriter, Write},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Sessions,
    ModelUsage,
    WeeklyCycles,
    ProjectTotals,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub kind: Kind,
    pub destination: String,
}

#[derive(Debug, Serialize)]
pub struct Outcome {
    pub path: String,
    pub row_count: u64,
}

#[derive(Debug, Serialize, thiserror::Error, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Error {
    #[error("Another CSV export is still running")]
    Busy,
    #[error("Choose an absolute path for a new CSV file")]
    InvalidDestination,
    #[error("That file already exists; choose a new filename")]
    DestinationExists,
    #[error("The CSV file could not be created or written; check its folder and permissions")]
    Write,
    #[error("The local usage database could not be read")]
    Storage,
}

impl From<rusqlite::Error> for Error {
    fn from(_: rusqlite::Error) -> Self {
        Self::Storage
    }
}
impl From<weekly::ReadError> for Error {
    fn from(_: weekly::ReadError) -> Self {
        Self::Storage
    }
}
impl From<std::io::Error> for Error {
    fn from(_: std::io::Error) -> Self {
        Self::Write
    }
}

/// Call on a blocking worker. Existing files, including logs and database files,
/// are never replaced. A failed export removes only its newly created output.
pub fn export_csv(database: &Path, request: Request) -> Result<Outcome, Error> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::Storage)?;
    export_at(
        database,
        request,
        Time {
            seconds: i64::try_from(now.as_secs()).map_err(|_| Error::Storage)?,
            nanos: now.subsec_nanos(),
        },
    )
}

fn export_at(database: &Path, request: Request, now: Time) -> Result<Outcome, Error> {
    let destination = Path::new(&request.destination);
    if !destination.is_absolute()
        || destination.file_stem().is_none()
        || !destination
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("csv"))
    {
        return Err(Error::InvalidDestination);
    }
    let mut connection = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    connection.busy_timeout(std::time::Duration::from_secs(3))?;
    // SQLite may spill grouping/sorting to disk; Rust retains just one output row.
    connection.execute_batch("PRAGMA temp_store=FILE; PRAGMA cache_size=-4096;")?;
    weekly_storage::register(&connection)?;
    let snapshot = connection.transaction()?;
    // Establish the snapshot before filesystem work, including on empty exports.
    snapshot.query_row("SELECT COUNT(*) FROM sessions", [], |_| Ok(()))?;
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                Error::DestinationExists
            } else {
                Error::Write
            }
        })?;
    let mut output = BufWriter::with_capacity(64 * 1024, file);
    let result = write_report(&snapshot, request.kind, now, &mut output).and_then(|rows| {
        output.flush()?;
        output.get_ref().sync_all()?;
        Ok(rows)
    });
    drop(output);
    if result.is_err() {
        let _ = fs::remove_file(destination);
    }
    result.map(|row_count| Outcome {
        path: request.destination,
        row_count,
    })
}

fn write_report(
    snapshot: &Connection,
    kind: Kind,
    now: Time,
    output: &mut impl Write,
) -> Result<u64, Error> {
    match kind {
        Kind::WeeklyCycles => write_weekly(snapshot, now, output),
        _ => write_usage(snapshot, kind, output),
    }
}

const CATEGORIES: [&str; 6] = [
    "total_tokens",
    "input_tokens",
    "cached_input_tokens",
    "cache_write_tokens",
    "output_tokens",
    "reasoning_tokens",
];
const USAGE_NOTE: &str = "Locally observed accepted direct usage; each session counted once. Cached input and reasoning overlap other categories; do not add categories. Estimated token cost is not an actual charge.";

fn token_headers(fields: &mut Vec<String>) {
    for category in CATEGORIES {
        fields.push(format!("{category}_known_subtotal"));
        fields.push(format!("{category}_state"));
    }
}

fn token_values(fields: &mut Vec<String>, tokens: Option<&Tokens>) {
    let categories = tokens.map(|tokens| {
        [
            &tokens.total_tokens,
            &tokens.input_tokens,
            &tokens.cached_input_tokens,
            &tokens.cache_write_tokens,
            &tokens.output_tokens,
            &tokens.reasoning_tokens,
        ]
    });
    for index in 0..CATEGORIES.len() {
        let category = categories.as_ref().map(|values| values[index]);
        fields.push(
            category
                .and_then(|category| category.known_tokens.clone())
                .unwrap_or_default(),
        );
        fields.push(category_state(category).into());
    }
}

fn category_state(category: Option<&Category>) -> &'static str {
    match category {
        Some(category) if category.complete => "complete",
        Some(category) if category.known_tokens.is_some() => "incomplete",
        _ => "unavailable",
    }
}

fn cost_state(known: Option<&str>, complete: bool, accepted: i64) -> &'static str {
    if complete {
        "complete"
    } else if known.is_some() {
        "incomplete_unpriced_usage"
    } else if accepted > 0 {
        "unpriced"
    } else {
        "unavailable"
    }
}

fn write_usage(snapshot: &Connection, kind: Kind, output: &mut impl Write) -> Result<u64, Error> {
    let (key, label) = match kind {
        Kind::Sessions => ("s.thread_id", "session_id"),
        Kind::ProjectTotals => ("s.project_id", "project_id"),
        Kind::ModelUsage => (
            "CASE WHEN o.model IS NULL THEN 'unknown:' ELSE 'model:' || o.model END",
            "model_id",
        ),
        Kind::WeeklyCycles => unreachable!(),
    };
    let session_fields = matches!(kind, Kind::Sessions);
    let mut headers = vec![
        label.into(),
        "attribution_basis".into(),
        "attribution_value".into(),
    ];
    if session_fields {
        headers.extend(["project_id", "project_basis", "project_value"].map(String::from));
    }
    headers.extend(
        [
            "usage_scope",
            "observed_sessions",
            "unavailable_sessions",
            "accepted_observations",
            "usage_state",
            "unresolved_usage",
            "source_diagnostics",
        ]
        .map(String::from),
    );
    token_headers(&mut headers);
    headers.extend(
        [
            "estimated_cost_known_subtotal_usd",
            "estimated_cost_state",
            "coverage_note",
        ]
        .map(String::from),
    );
    csv_row(output, &headers)?;

    // Clear rejected fields in SQL before reusing the accepted-category projection.
    // A single grouped cursor avoids loading history or querying every session in Rust.
    let sql = format!(
        "WITH {}, usage AS (
          SELECT id,thread_id,model,accepted,
            CASE WHEN accepted=1 THEN total END AS total,
            CASE WHEN accepted=1 THEN normalized END AS normalized
          FROM observations
        ), source_coverage AS (
          SELECT thread_id,MAX(diagnostic IS NOT NULL OR legacy=1 OR halted=1 OR partial=1) AS diagnostic
          FROM sources GROUP BY thread_id
        )
        SELECT {key} AS group_id,s.project_id,
          COUNT(DISTINCT s.thread_id),COUNT(CASE WHEN o.accepted=1 THEN 1 END),
          MAX(s.incomplete),COALESCE(MAX(o.accepted=0),0),COALESCE(MAX(sc.diagnostic),0),
          {},estimated_cost_sum(v.amount),COUNT(v.observation_id),
          COUNT(DISTINCT CASE WHEN o.accepted=1 THEN o.thread_id END)
        FROM project_sessions s
        LEFT JOIN usage o ON o.thread_id=s.thread_id
        LEFT JOIN observation_valuations v ON v.observation_id=o.id AND o.accepted=1
        LEFT JOIN source_coverage sc ON sc.thread_id=s.thread_id
        WHERE s.is_placeholder=0 GROUP BY group_id ORDER BY group_id COLLATE BINARY",
        aggregates::PROJECTS,
        aggregates::token_fields(),
    );
    let mut statement = snapshot.prepare(&sql)?;
    let mut rows = statement.query([])?;
    let mut count = 0;
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let attribution = aggregates::attribution(id.clone());
        let mut fields = if session_fields {
            let project = aggregates::attribution(row.get(1)?);
            vec![
                id,
                "observed_session".into(),
                String::new(),
                project.id,
                project.basis,
                project.value.unwrap_or_default(),
            ]
        } else {
            vec![id, attribution.basis, attribution.value.unwrap_or_default()]
        };
        let accepted: i64 = row.get(3)?;
        let incomplete: bool = row.get(4)?;
        let unresolved: bool = row.get(5)?;
        let source_diagnostics: bool = row.get(6)?;
        let observed_sessions: i64 = row.get(2)?;
        let unavailable_sessions = observed_sessions - row.get::<_, i64>(21)?;
        fields.extend([
            "direct".into(),
            observed_sessions.to_string(),
            unavailable_sessions.to_string(),
            accepted.to_string(),
            if accepted == 0 {
                "unavailable"
            } else if incomplete || unresolved || source_diagnostics || unavailable_sessions > 0 {
                "incomplete"
            } else {
                "observed"
            }
            .into(),
            unresolved.to_string(),
            source_diagnostics.to_string(),
        ]);
        let tokens = aggregates::row_tokens(row, 7, accepted)?;
        token_values(&mut fields, Some(&tokens));
        let amount: Option<String> = row.get(19)?;
        let complete = accepted > 0 && accepted == row.get::<_, i64>(20)?;
        fields.push(usd(amount.as_deref())?);
        fields.push(cost_state(amount.as_deref(), complete, accepted).into());
        fields.push(USAGE_NOTE.into());
        csv_row(output, &fields)?;
        count += 1;
    }
    Ok(count)
}

fn write_weekly(snapshot: &Connection, now: Time, output: &mut impl Write) -> Result<u64, Error> {
    let mut headers = [
        "cycle_key",
        "cycle_state",
        "usage_scope",
        "first_observed_unix_seconds",
        "last_observed_unix_seconds",
        "first_used_percent",
        "last_used_percent",
        "last_remaining_percent",
        "resets_at_unix_seconds",
        "detected_reset",
        "ambiguous_observations",
        "full_cycle_cost_state",
        "interval_start_exclusive_unix_seconds",
        "interval_end_inclusive_unix_seconds",
        "consumed_percentage_points",
        "estimated_cost_known_subtotal_usd",
        "estimated_cost_state",
        "observed_usd_per_percentage_point",
        "estimated_full_week_usd",
        "estimate_state",
    ]
    .map(String::from)
    .to_vec();
    token_headers(&mut headers);
    headers.push("coverage_note".into());
    csv_row(output, &headers)?;
    let mut timeline = Timeline::new(now, None, 0);
    let mut previous: Option<CompletedCycle> = None;
    let mut count = 0;
    let mut write_error = None;
    let scan = weekly_storage::scan(snapshot, now, &mut timeline, |timeline, _| {
        if let (Some(before), Some(current)) = (&previous, &timeline.current) {
            if before.cycle.key != current.key {
                if let Err(error) = write_cycle(snapshot, before, "completed", output) {
                    write_error = Some(error);
                    return Err(weekly::ReadError::Storage);
                }
                count += 1;
            }
        }
        previous = timeline.current.as_ref().map(|cycle| CompletedCycle {
            cycle: cycle.clone(),
            baseline: timeline.baseline.clone(),
            latest: timeline.latest.clone(),
            ambiguous: timeline.ambiguous,
        });
        Ok(())
    });
    if let Some(error) = write_error {
        return Err(error);
    }
    scan?;
    if let Some(current) = previous {
        write_cycle(snapshot, &current, "current", output)?;
        count += 1;
    }
    Ok(count)
}

fn write_cycle(
    snapshot: &Connection,
    completed: &CompletedCycle,
    state: &str,
    output: &mut impl Write,
) -> Result<(), Error> {
    let cycle = &completed.cycle;
    let estimate = weekly_storage::estimate(
        snapshot,
        completed.baseline.as_ref(),
        completed.latest.as_ref(),
        completed.ambiguous,
    )?;
    let tokens = match (estimate.start, estimate.end) {
        (Some(start), Some(end)) => Some(weekly_storage::tokens(snapshot, start, end)?),
        _ => None,
    };
    let cost = estimate.estimated_cost.as_ref();
    let known = cost.and_then(|cost| cost.known_subtotal.as_deref());
    let reason = match estimate.unavailable_reason {
        None => "available",
        Some(weekly::Unavailable::InsufficientObservations) => "insufficient_observations",
        Some(weekly::Unavailable::AmbiguousObservation) => "ambiguous_observation",
        Some(weekly::Unavailable::BelowOnePercentagePoint) => "below_one_percentage_point",
        Some(weekly::Unavailable::UnpricedUsage) => "unpriced_usage",
    };
    let mut fields = vec![
        cycle.key.clone(),
        state.into(),
        "global_direct_comparable_interval".into(),
        unix_seconds(cycle.first_observation.time),
        unix_seconds(cycle.last_observation.time),
        cycle.first_observation.used_percent.clone(),
        cycle.last_observation.used_percent.clone(),
        cycle.last_observation.remaining_percent.clone(),
        cycle
            .last_observation
            .resets_at
            .map(|value| value.to_string())
            .unwrap_or_default(),
        cycle.detected_reset.to_string(),
        cycle.has_ambiguous_observations.to_string(),
        if cycle.full_cycle_cost_known {
            "complete"
        } else {
            "unavailable"
        }
        .into(),
        estimate.start.map(unix_seconds).unwrap_or_default(),
        estimate.end.map(unix_seconds).unwrap_or_default(),
        estimate.consumed_percentage_points.unwrap_or_default(),
        usd(known)?,
        cost_state(
            known,
            cost.is_some_and(|cost| cost.complete),
            cost.map_or(0, |cost| cost.accepted_observations as i64),
        )
        .into(),
        estimate.effective_usd_per_percent.unwrap_or_default(),
        estimate.estimated_full_week_usd.unwrap_or_default(),
        reason.into(),
    ];
    token_values(&mut fields, tokens.as_ref());
    fields.push("Observed local estimated token cost for the comparable interval only; newer unmatched usage and unobserved account usage are excluded. Full-cycle cost and session weekly percentage impact are unavailable. Token categories overlap; do not add categories.".into());
    csv_row(output, &fields)
}

fn usd(amount: Option<&str>) -> Result<String, Error> {
    amount
        .map(|encoded| {
            let amount = encoded.parse::<i128>().map_err(|_| Error::Storage)?;
            if amount < 0 || amount.to_string() != encoded {
                return Err(Error::Storage);
            }
            Ok(format!(
                "{}.{:012}",
                amount / 1_000_000_000_000,
                amount % 1_000_000_000_000
            ))
        })
        .transpose()
        .map(|value| value.unwrap_or_default())
}

fn unix_seconds(time: Time) -> String {
    let nanos = i128::from(time.seconds) * 1_000_000_000 + i128::from(time.nanos);
    let magnitude = nanos.unsigned_abs();
    format!(
        "{}{}.{:09}",
        if nanos < 0 { "-" } else { "" },
        magnitude / 1_000_000_000,
        magnitude % 1_000_000_000
    )
}

fn csv_row(output: &mut impl Write, fields: &[String]) -> Result<(), Error> {
    for (index, value) in fields.iter().enumerate() {
        if index > 0 {
            output.write_all(b",")?;
        }
        if value.contains([',', '"', '\r', '\n']) {
            output.write_all(b"\"")?;
            for (index, part) in value.split('"').enumerate() {
                if index > 0 {
                    output.write_all(b"\"\"")?;
                }
                output.write_all(part.as_bytes())?;
            }
            output.write_all(b"\"")?;
        } else {
            output.write_all(value.as_bytes())?;
        }
    }
    output.write_all(b"\r\n")?;
    Ok(())
}

#[cfg(test)]
mod tests;
