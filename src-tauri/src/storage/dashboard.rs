//! A single read snapshot for cards, global direct usage, and bounded chart data.
use super::{aggregates, weekly, Store};
use crate::{
    dashboard::{self as dto, Downsample, Projection},
    weekly::{Cost, ReadError, Time, Timeline},
};
use rusqlite::{params, Connection, Rows};
use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
mod categories;
mod models;
mod quota;
mod turns;
mod usage;

impl Store {
    pub fn read_dashboard(path: &Path, query: dto::Query) -> Result<dto::Response, ReadError> {
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
        Self { connection }.dashboard_at(
            query,
            Time {
                seconds: i64::try_from(now.as_secs()).map_err(|_| ReadError::Storage)?,
                nanos: now.subsec_nanos(),
            },
        )
    }

    pub(crate) fn dashboard_at(
        &mut self,
        query: dto::Query,
        now: Time,
    ) -> Result<dto::Response, ReadError> {
        let bins = query.validate()?;
        weekly::register(&self.connection)?;
        let tx = self
            .connection
            .transaction()
            .map_err(|_| ReadError::Storage)?;
        let (weekly, earliest) = weekly::project_with_start(
            &tx,
            crate::weekly::Query {
                before: None,
                limit: 1,
            },
            now,
        )?;
        let global = aggregates::global_summary(&tx).map_err(|_| ReadError::Storage)?;
        let start = query.start(
            now,
            weekly
                .current_cycle
                .as_ref()
                .map(|cycle| cycle.first_observation.time),
            earliest,
        );
        let (local_usage, breakdowns, turn_activity) = usage::read(
            &tx,
            &query,
            weekly
                .current_cycle
                .as_ref()
                .map(|cycle| cycle.first_observation.time),
            now,
            bins as u32,
        )?;
        let mut downsample = Downsample::new(query.range, start, now, bins);
        let mut projection = Projection::default();
        let mut timeline = Timeline::new(now, None, 1);
        // One SQL grouping/sort and one chronological merge, never a prefix cost
        // query per quota observation. Immutable valuations use the existing exact sum.
        let mut statement = tx.prepare(COST_GROUPS).map_err(|_| ReadError::Storage)?;
        let mut rows = statement
            .query(params![now.seconds, now.nanos])
            .map_err(|_| ReadError::Storage)?;
        let mut next = next_cost(&mut rows)?;
        weekly::scan(&tx, now, &mut timeline, |timeline, time| {
            let mut amount = 0i128;
            let mut known = false;
            let mut accepted = 0u64;
            let mut complete = true;
            while next
                .as_ref()
                .is_some_and(|(cost_time, _)| *cost_time <= time)
            {
                let (_, cost) = next.take().unwrap();
                if let Some(value) = cost.known_subtotal {
                    amount = amount
                        .checked_add(value.parse::<i128>().map_err(|_| ReadError::Storage)?)
                        .ok_or(ReadError::Storage)?;
                    known = true;
                }
                accepted = accepted
                    .checked_add(cost.accepted_observations)
                    .ok_or(ReadError::Storage)?;
                complete &= cost.complete;
                next = next_cost(&mut rows)?;
            }
            let interval = Cost {
                known_subtotal: (known || accepted == 0).then(|| amount.to_string()),
                complete,
                accepted_observations: accepted,
            };
            let (point, boundary) = projection.push(timeline, time, interval)?;
            downsample.push(point, boundary);
            Ok(())
        })?;
        let quota_analysis = quota::read(&tx, start, now)?;
        Ok(dto::Response { evaluated_at: now, weekly, global,
            token_scope: "All locally observed history; direct session usage counted once. Cached input and reasoning overlap other categories; do not add categories.",
            chart: downsample.finish(), local_usage, breakdowns, turn_activity, quota_analysis })
    }
}

const COST_GROUPS: &str = "SELECT o.time_seconds,o.time_nanos,estimated_cost_sum(v.amount),COUNT(*),COUNT(v.observation_id) FROM observations o LEFT JOIN observation_valuations v ON v.observation_id=o.id WHERE o.accepted=1 AND o.time_seconds IS NOT NULL AND o.time_nanos IS NOT NULL AND (o.time_seconds,o.time_nanos)<=(?1,?2) GROUP BY o.time_seconds,o.time_nanos ORDER BY o.time_seconds,o.time_nanos";

fn next_cost(rows: &mut Rows<'_>) -> Result<Option<(Time, Cost)>, ReadError> {
    let Some(row) = rows.next().map_err(|_| ReadError::Storage)? else {
        return Ok(None);
    };
    let accepted: i64 = row.get(3).map_err(|_| ReadError::Storage)?;
    let priced: i64 = row.get(4).map_err(|_| ReadError::Storage)?;
    Ok(Some((
        Time {
            seconds: row.get(0).map_err(|_| ReadError::Storage)?,
            nanos: row.get(1).map_err(|_| ReadError::Storage)?,
        },
        Cost {
            known_subtotal: row.get(2).map_err(|_| ReadError::Storage)?,
            complete: accepted == priced,
            accepted_observations: accepted.try_into().map_err(|_| ReadError::Storage)?,
        },
    )))
}

#[cfg(test)]
mod tests;
