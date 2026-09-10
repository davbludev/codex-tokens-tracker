//! Consistent read-only projection over retained quota samples and immutable costs.
use super::{
    aggregates::{row_tokens, token_fields, CostSum},
    Store,
};
use crate::{
    adapter,
    aggregates::Tokens,
    weekly::{self as dto, Cost, Estimate, ReadError, Sample, Time, Timeline, Unavailable},
};
use rusqlite::{functions::FunctionFlags, params, Connection};
use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

impl Store {
    pub fn read_weekly_models(
        path: &Path,
        query: dto::ModelsQuery,
    ) -> Result<Option<dto::Models>, ReadError> {
        query.validate()?;
        let connection =
            Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(|_| ReadError::Storage)?;
        connection
            .busy_timeout(std::time::Duration::from_secs(3))
            .map_err(|_| ReadError::Storage)?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| ReadError::Storage)?;
        Self { connection }.weekly_models_at(
            query,
            Time {
                seconds: i64::try_from(now.as_secs()).map_err(|_| ReadError::Storage)?,
                nanos: now.subsec_nanos(),
            },
        )
    }

    pub(crate) fn weekly_models_at(
        &mut self,
        query: dto::ModelsQuery,
        now: Time,
    ) -> Result<Option<dto::Models>, ReadError> {
        let key = query.validate()?;
        register(&self.connection)?;
        let tx = self
            .connection
            .transaction()
            .map_err(|_| ReadError::Storage)?;
        // Retain at most the requested cycle and one predecessor, even for old keys.
        let before = if key.nanos < 999_999_999 {
            Some(Time {
                nanos: key.nanos + 1,
                ..key
            })
        } else {
            key.seconds
                .checked_add(1)
                .map(|seconds| Time { seconds, nanos: 0 })
        };
        let mut timeline = Timeline::new(now, before, 1);
        scan(&tx, now, &mut timeline, |_, _| Ok(()))?;
        let (cycles, _) = timeline.finish();
        let Some(cycle) = cycles
            .into_iter()
            .find(|cycle| cycle.cycle.key == query.cycle_key)
        else {
            return Ok(None);
        };
        let estimate = estimate(
            &tx,
            cycle.baseline.as_ref(),
            cycle.latest.as_ref(),
            cycle.ambiguous,
        )?;
        let (items, next_cursor) = match (estimate.start, estimate.end) {
            (Some(start), Some(end)) => models(&tx, start, end, &query.page)?,
            _ => (Vec::new(), None),
        };
        Ok(Some(dto::Models {
            cycle_key: query.cycle_key,
            estimate,
            items,
            next_cursor,
        }))
    }

    pub fn read_weekly(path: &Path, query: dto::Query) -> Result<dto::Response, ReadError> {
        query.validate()?;
        let connection =
            Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(|_| ReadError::Storage)?;
        connection
            .busy_timeout(std::time::Duration::from_secs(3))
            .map_err(|_| ReadError::Storage)?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| ReadError::Storage)?;
        Self { connection }.weekly_at(
            query,
            Time {
                seconds: i64::try_from(now.as_secs()).map_err(|_| ReadError::Storage)?,
                nanos: now.subsec_nanos(),
            },
        )
    }

    pub(crate) fn weekly_at(
        &mut self,
        query: dto::Query,
        now: Time,
    ) -> Result<dto::Response, ReadError> {
        query.validate()?;
        register(&self.connection)?;
        let tx = self
            .connection
            .transaction()
            .map_err(|_| ReadError::Storage)?;
        project(&tx, query, now)
    }
}

pub(crate) fn register(connection: &Connection) -> Result<(), ReadError> {
    let flags = FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC;
    connection
        .create_aggregate_function("estimated_cost_sum", 1, flags, CostSum)
        .map_err(|_| ReadError::Storage)?;
    connection
        .create_scalar_function("weekly_seconds", 1, flags, |ctx| {
            let value: Option<String> = ctx.get(0)?;
            Ok(value
                .and_then(|value| adapter::observation_time(&value).ok())
                .map(|time| time.0))
        })
        .map_err(|_| ReadError::Storage)?;
    connection
        .create_scalar_function("weekly_nanos", 1, flags, |ctx| {
            let value: Option<String> = ctx.get(0)?;
            Ok(value
                .and_then(|value| adapter::observation_time(&value).ok())
                .map(|time| time.1))
        })
        .map_err(|_| ReadError::Storage)?;
    Ok(())
}

pub(super) fn project(
    tx: &Connection,
    query: dto::Query,
    now: Time,
) -> Result<dto::Response, ReadError> {
    project_with_start(tx, query, now).map(|(response, _)| response)
}

pub(super) fn project_with_start(
    tx: &Connection,
    query: dto::Query,
    now: Time,
) -> Result<(dto::Response, Option<Time>), ReadError> {
    let before = query.validate()?;
    let mut timeline = Timeline::new(now, before, query.limit);
    let mut earliest = None;
    let excluded_samples = scan(tx, now, &mut timeline, |_, time| {
        earliest.get_or_insert(time);
        Ok(())
    })?;
    let (history, next_cursor) = timeline.finish();
    let history = history
        .into_iter()
        .map(|completed| {
            let estimate = estimate(
                tx,
                completed.baseline.as_ref(),
                completed.latest.as_ref(),
                completed.ambiguous,
            )?;
            let tokens = match (estimate.start, estimate.end) {
                (Some(start), Some(end)) => Some(tokens(tx, start, end)?),
                _ => None,
            };
            Ok(dto::HistoricalCycle {
                cycle: completed.cycle,
                estimate,
                tokens,
            })
        })
        .collect::<Result<Vec<_>, ReadError>>()?;
    let estimate = |start: Option<&Sample>| -> Result<Estimate, ReadError> {
        estimate(tx, start, timeline.latest.as_ref(), timeline.ambiguous)
    };
    let overall = estimate(timeline.baseline.as_ref())?;
    let recent = estimate(timeline.recent_start.as_ref())?;
    let latest = timeline.latest.as_ref().map(|sample| sample.time);
    let unmatched_cost = latest.map(|start| cost(tx, start, now)).transpose()?;
    let observation_age_seconds = latest.map(|latest| {
        let seconds = now.seconds.saturating_sub(latest.seconds);
        seconds.saturating_sub(i64::from(now.nanos < latest.nanos)) as u64
    });
    Ok((dto::Response { evaluated_at: now, current_cycle: timeline.current, observation_age_seconds,
            overall, recent, unmatched_cost, unmatched_cost_start: latest, history, next_cursor,
            excluded_samples, stale_samples: timeline.stale, session_weekly_percentage_impact: None,
            coverage_note: "Since observation began: observed local estimated token cost for the current model mix, not an OpenAI charge or proof of complete account usage. Cost uses start-exclusive/end-inclusive comparable observation intervals; newer unmatched cost is separate. Session weekly percentage impact is unavailable.",
        }, earliest))
}

pub(crate) fn estimate(
    tx: &Connection,
    start: Option<&Sample>,
    end: Option<&Sample>,
    ambiguous: bool,
) -> Result<Estimate, ReadError> {
    if ambiguous {
        return Ok(Estimate::unavailable(Unavailable::AmbiguousObservation));
    }
    match (start, end) {
        (Some(start), Some(end)) if start.time < end.time => Ok(Estimate::matched(
            start,
            end,
            cost(tx, start.time, end.time)?,
        )),
        _ => Ok(Estimate::unavailable(Unavailable::InsufficientObservations)),
    }
}

const INTERVAL: &str = "o.accepted=1 AND (o.time_seconds,o.time_nanos)>(?1,?2) AND (o.time_seconds,o.time_nanos)<=(?3,?4)";

pub(crate) fn tokens(tx: &Connection, start: Time, end: Time) -> Result<Tokens, ReadError> {
    tx.query_row(
        &format!(
            "SELECT COUNT(*),{} FROM observations o WHERE {INTERVAL}",
            token_fields()
        ),
        params![start.seconds, start.nanos, end.seconds, end.nanos],
        |row| row_tokens(row, 1, row.get(0)?),
    )
    .map_err(|_| ReadError::Storage)
}

fn models(
    tx: &Connection,
    start: Time,
    end: Time,
    page: &crate::aggregates::PageRequest,
) -> Result<(Vec<dto::Model>, Option<String>), ReadError> {
    let mut statement = tx.prepare(&format!("SELECT CASE WHEN o.model IS NULL THEN 'unknown:' ELSE 'model:'||o.model END AS model_id,o.model,COUNT(*),{},estimated_cost_sum(v.amount),COUNT(v.observation_id) FROM observations o LEFT JOIN observation_valuations v ON v.observation_id=o.id WHERE {INTERVAL} AND (?5 IS NULL OR (CASE WHEN o.model IS NULL THEN 'unknown:' ELSE 'model:'||o.model END) COLLATE BINARY > ?5) GROUP BY o.model ORDER BY model_id COLLATE BINARY LIMIT ?6", token_fields())).map_err(|_| ReadError::Storage)?;
    let rows = statement
        .query_map(
            params![
                start.seconds,
                start.nanos,
                end.seconds,
                end.nanos,
                page.after,
                page.limit + 1
            ],
            |row| {
                let accepted: i64 = row.get(2)?;
                let priced: i64 = row.get(16)?;
                Ok(dto::Model {
                    id: row.get(0)?,
                    model: row.get(1)?,
                    tokens: row_tokens(row, 3, accepted)?,
                    estimated_cost: Cost {
                        known_subtotal: row.get(15)?,
                        complete: accepted == priced,
                        accepted_observations: accepted as u64,
                    },
                })
            },
        )
        .map_err(|_| ReadError::Storage)?;
    let mut items = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ReadError::Storage)?;
    let more = items.len() > page.limit as usize;
    if more {
        items.pop();
    }
    let next = more.then(|| items.last().unwrap().id.clone());
    Ok((items, next))
}

/// Visit completed canonical time groups. Both readers use the same quota
/// reducer, including its conflict barrier and reset-metadata tie handling.
pub(crate) fn scan(
    connection: &Connection,
    now: Time,
    timeline: &mut Timeline,
    mut visit: impl FnMut(&Timeline, Time) -> Result<(), ReadError>,
) -> Result<u64, ReadError> {
    let mut excluded = 0;
    let mut group_time = None;
    let mut statement = connection.prepare("SELECT weekly_seconds(timestamp) AS seconds,weekly_nanos(timestamp) AS nanos,used_percent,resets_at FROM limit_samples WHERE bucket='codex' AND window_minutes=10080 ORDER BY seconds,nanos")
        .map_err(|_| ReadError::Storage)?;
    let mut rows = statement.query([]).map_err(|_| ReadError::Storage)?;
    while let Some(row) = rows.next().map_err(|_| ReadError::Storage)? {
        let seconds: Option<i64> = row.get(0).map_err(|_| ReadError::Storage)?;
        let nanos: Option<u32> = row.get(1).map_err(|_| ReadError::Storage)?;
        let encoded: Option<String> = row.get(2).map_err(|_| ReadError::Storage)?;
        if let (Some(seconds), Some(nanos), Some(used)) =
            (seconds, nanos, encoded.as_deref().and_then(dto::percentage))
        {
            let time = Time { seconds, nanos };
            if time <= now {
                if let Some(previous) = group_time.filter(|previous| *previous != time) {
                    timeline.flush();
                    visit(timeline, previous)?;
                }
                timeline.push(Sample {
                    time,
                    used,
                    reset: row.get(3).map_err(|_| ReadError::Storage)?,
                });
                group_time = Some(time);
                continue;
            }
        }
        excluded += 1;
    }
    if let Some(time) = group_time {
        timeline.flush();
        visit(timeline, time)?;
    }
    Ok(excluded)
}

fn cost(connection: &Connection, start: Time, end: Time) -> Result<Cost, ReadError> {
    connection.query_row("SELECT estimated_cost_sum(v.amount),COUNT(*),COUNT(v.observation_id) FROM observations o LEFT JOIN observation_valuations v ON v.observation_id=o.id WHERE o.accepted=1 AND (o.time_seconds,o.time_nanos)>(?1,?2) AND (o.time_seconds,o.time_nanos)<=(?3,?4)",
        params![start.seconds,start.nanos,end.seconds,end.nanos], |row| {
            let amount: Option<String> = row.get(0)?;
            let accepted: i64 = row.get(1)?;
            let priced: i64 = row.get(2)?;
            Ok(Cost { known_subtotal: if accepted == 0 { Some("0".into()) } else { amount }, complete: accepted == priced, accepted_observations: accepted as u64 })
        }).map_err(|_| ReadError::Storage)
}
