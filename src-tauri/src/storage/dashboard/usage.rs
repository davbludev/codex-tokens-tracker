//! Bounded local activity and ranked composition, independent of quota availability.
use crate::{
    aggregates::{Category, EstimatedCost},
    dashboard::{self as dto, BreakdownMetric, Range},
    storage::aggregates::{row_tokens, token_fields, PROJECTS},
    weekly::{ReadError, Time},
};
use rusqlite::{functions::FunctionFlags, params, OptionalExtension, Row, Transaction};

const INTERVAL: &str = "o.accepted=1 AND o.time_seconds IS NOT NULL AND o.time_nanos IS NOT NULL AND (o.time_seconds,o.time_nanos)>(?1,?2) AND (o.time_seconds,o.time_nanos)<=(?3,?4)";

fn nanos(time: Time) -> i128 {
    i128::from(time.seconds) * 1_000_000_000 + i128::from(time.nanos)
}
fn time(value: i128) -> Time {
    Time {
        seconds: value.div_euclid(1_000_000_000) as i64,
        nanos: value.rem_euclid(1_000_000_000) as u32,
    }
}

pub(super) fn read(
    tx: &Transaction<'_>,
    query: &dto::Query,
    cycle: Option<Time>,
    now: Time,
    bins: u32,
) -> Result<(dto::LocalUsage, dto::Breakdowns), ReadError> {
    let start = match query.range {
        Range::All => tx.query_row(
            "SELECT time_seconds,time_nanos FROM observations WHERE accepted=1 AND time_seconds IS NOT NULL AND time_nanos IS NOT NULL AND (time_seconds,time_nanos)<=(?1,?2) ORDER BY time_seconds,time_nanos LIMIT 1",
            params![now.seconds, now.nanos],
            |row| Ok(Time { seconds: row.get(0)?, nanos: row.get(1)? }),
        ).optional().map_err(|_| ReadError::Storage)?.map(|first| time(nanos(first) - 1)).unwrap_or(now),
        Range::CurrentCycle if cycle.is_none() => Time { seconds: now.seconds.saturating_sub(7 * 86400), nanos: now.nanos },
        _ => query.start(now, cycle, None),
    };
    let width = (nanos(now) - nanos(start)).max(1);
    tx.create_scalar_function(
        "dashboard_usage_bin",
        2,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        move |ctx| {
            let observed = Time {
                seconds: ctx.get(0)?,
                nanos: ctx.get(1)?,
            };
            Ok(((nanos(observed) - nanos(start)) * i128::from(bins) - 1).div_euclid(width) as i64)
        },
    )
    .map_err(|_| ReadError::Storage)?;
    let parameters = params![start.seconds, start.nanos, now.seconds, now.nanos];
    let fields = format!("COUNT(*),{},estimated_cost_sum(v.amount),COUNT(v.observation_id),COUNT(DISTINCT o.thread_id)", token_fields());
    let from = format!("FROM observations o LEFT JOIN observation_valuations v ON v.observation_id=o.id WHERE {INTERVAL}");
    let summary = tx
        .query_row(&format!("SELECT {fields} {from}"), parameters, |row| {
            read_summary(row, 0)
        })
        .map_err(|_| ReadError::Storage)?;
    let mut statement = tx.prepare(&format!("SELECT dashboard_usage_bin(o.time_seconds,o.time_nanos) AS bin,{fields} {from} GROUP BY bin ORDER BY bin LIMIT {bins}")).map_err(|_| ReadError::Storage)?;
    let points = statement
        .query_map(parameters, |row| {
            let index: u32 = row.get(0)?;
            let bound =
                |index: u32| time(nanos(start) + width * i128::from(index) / i128::from(bins));
            Ok(dto::UsagePoint {
                index,
                start: bound(index),
                end: bound(index + 1),
                summary: read_summary(row, 1)?,
            })
        })
        .map_err(|_| ReadError::Storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|_| ReadError::Storage)?;
    let untimed_observations = tx.query_row("SELECT COUNT(*) FROM observations WHERE accepted=1 AND (time_seconds IS NULL OR time_nanos IS NULL)", [], |row| row.get::<_, i64>(0)).map_err(|_| ReadError::Storage)? as u64;
    let breakdowns = dto::Breakdowns {
        metric: query.breakdown_metric,
        models: breakdown(tx, start, now, query.breakdown_metric, true)?,
        projects: breakdown(tx, start, now, query.breakdown_metric, false)?,
    };
    Ok((dto::LocalUsage {
        start, end: now, bin_count: bins, summary, points, untimed_observations,
        coverage_note: "Accepted direct local usage in start-exclusive/end-inclusive bins. Empty bins contain no accepted observations, not measured complete zero. Untimed usage cannot be assigned to this range. Token categories overlap; do not add them. Unpriced usage remains unknown.",
    }, breakdowns))
}

fn read_summary(row: &Row<'_>, offset: usize) -> rusqlite::Result<dto::UsageSummary> {
    let accepted: i64 = row.get(offset)?;
    Ok(dto::UsageSummary {
        tokens: row_tokens(row, offset + 1, accepted)?,
        estimated_cost: EstimatedCost {
            known_subtotal: row.get(offset + 13)?,
            complete: accepted > 0 && accepted == row.get::<_, i64>(offset + 14)?,
        },
        observed_sessions: row.get::<_, i64>(offset + 15)? as u64,
    })
}

fn breakdown(
    tx: &Transaction<'_>,
    start: Time,
    end: Time,
    metric: BreakdownMetric,
    models: bool,
) -> Result<Vec<dto::Breakdown>, ReadError> {
    let key = if models {
        "CASE WHEN o.model IS NULL THEN 'unknown:' ELSE 'model:'||o.model END"
    } else {
        "o.project_id"
    };
    // Immutable cost sums are canonical nonnegative integers: length then lexical
    // ordering retains precision above both SQLite integer and JS number ranges.
    let ordering = match metric {
        BreakdownMetric::Tokens => "total DESC,id",
        BreakdownMetric::Cost => "length(amount) DESC,amount DESC,id",
    };
    let sql = format!("WITH {PROJECTS}, usage AS (
        SELECT o.*,COALESCE(s.project_id,'unknown:') AS project_id FROM observations o
        LEFT JOIN project_sessions s ON s.thread_id=o.thread_id WHERE {INTERVAL}
    ), grouped AS (
        SELECT {key} AS id,COUNT(*) AS accepted,SUM(o.total) AS total,COUNT(o.total) AS known,
               estimated_cost_sum(v.amount) AS amount,COUNT(v.observation_id) AS priced
        FROM usage o LEFT JOIN observation_valuations v ON v.observation_id=o.id GROUP BY id
    ), ranked AS (
        SELECT *,ROW_NUMBER() OVER (PARTITION BY id='unknown:' ORDER BY {ordering}) AS position FROM grouped
    ), buckets AS (
        SELECT CASE WHEN id='unknown:' OR position<=5 THEN id ELSE 'other:' END AS id,
               SUM(accepted) AS accepted,SUM(total) AS total,SUM(known) AS known,
               estimated_cost_sum(amount) AS amount,SUM(priced) AS priced
        FROM ranked GROUP BY CASE WHEN id='unknown:' OR position<=5 THEN id ELSE 'other:' END
    ) SELECT id,accepted,total,known,amount,priced FROM buckets
      ORDER BY CASE id WHEN 'unknown:' THEN 1 WHEN 'other:' THEN 2 ELSE 0 END,{ordering} LIMIT 7");
    let mut statement = tx.prepare(&sql).map_err(|_| ReadError::Storage)?;
    let rows = statement
        .query_map(
            params![start.seconds, start.nanos, end.seconds, end.nanos],
            |row| {
                let key: String = row.get(0)?;
                let accepted: i64 = row.get(1)?;
                let (label, kind) = match key.as_str() {
                    "unknown:" => (
                        if models {
                            "Unknown model"
                        } else {
                            "Unattributed project"
                        }
                        .to_owned(),
                        "unknown",
                    ),
                    "other:" => (
                        if models {
                            "Other models"
                        } else {
                            "Other projects"
                        }
                        .to_owned(),
                        "other",
                    ),
                    _ if models => (
                        key.strip_prefix("model:").unwrap_or(&key).to_owned(),
                        "model",
                    ),
                    _ => {
                        let path = key.split_once(':').map(|(_, path)| path).unwrap_or(&key);
                        let mut path = path.trim_end_matches(['/', '\\']);
                        if key.starts_with("repository:")
                            && path.rsplit(['/', '\\']).next() == Some(".git")
                        {
                            path = path[..path.len() - 4].trim_end_matches(['/', '\\']);
                        }
                        (
                            path.rsplit(['/', '\\']).next().unwrap_or(path).to_owned(),
                            "project",
                        )
                    }
                };
                Ok(dto::Breakdown {
                    key,
                    label,
                    kind,
                    tokens: Category::from_sum(row.get(2)?, row.get(3)?, accepted),
                    estimated_cost: EstimatedCost {
                        known_subtotal: row.get(4)?,
                        complete: accepted > 0 && accepted == row.get::<_, i64>(5)?,
                    },
                })
            },
        )
        .map_err(|_| ReadError::Storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|_| ReadError::Storage);
    rows
}
