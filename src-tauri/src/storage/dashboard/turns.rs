//! Turns grouped by the combination of a turn's model and its reasoning effort.
//!
//! A turn is one `turn_id` within one session. Every turn is attributed to the
//! bin of its first accepted observation in the range and contributes all of its
//! tokens and cost there, so the per-bin counts, tokens and amounts each add up
//! exactly to the series totals a reader sees beside the chart.
use crate::{
    aggregates::{Category, EstimatedCost},
    dashboard as dto,
    weekly::{ReadError, Time},
};
use rusqlite::{functions::FunctionFlags, params, Transaction};
use std::collections::BTreeMap;

/// Named series kept before the remainder folds into one `other:` series.
const MAX_SERIES: usize = 7;
/// Enough resolution to see a working pattern without an unreadable bar width.
const MAX_BINS: u32 = 96;

#[derive(Default)]
struct Series {
    model: Option<String>,
    effort: Option<String>,
    turns: u64,
    accepted: i64,
    total: Option<i64>,
    known: i64,
    stored: i128,
    priced: i64,
    /// None once a fold makes the distinct set unrecoverable.
    sessions: Option<u64>,
    points: BTreeMap<u32, Point>,
}

#[derive(Default)]
struct Point {
    turns: u64,
    accepted: i64,
    total: Option<i64>,
    known: i64,
    stored: i128,
    priced: i64,
}

fn add(left: Option<i64>, right: Option<i64>) -> Option<i64> {
    match (left, right) {
        (Some(a), Some(b)) => Some(a.saturating_add(b)),
        (value, None) | (None, value) => value,
    }
}

impl Point {
    fn merge(&mut self, other: &Point) {
        self.turns += other.turns;
        self.accepted += other.accepted;
        self.total = add(self.total, other.total);
        self.known += other.known;
        self.stored = self.stored.saturating_add(other.stored);
        self.priced += other.priced;
    }
    fn finish(&self, index: u32) -> dto::TurnPoint {
        dto::TurnPoint {
            index,
            turns: self.turns,
            tokens: Category::from_sum(self.total, self.known, self.accepted),
            estimated_cost: EstimatedCost {
                known_subtotal: (self.priced > 0).then(|| self.stored.to_string()),
                complete: self.accepted > 0 && self.accepted == self.priced,
            },
        }
    }
}

impl Series {
    fn merge(&mut self, other: Series) {
        self.turns += other.turns;
        self.accepted += other.accepted;
        self.total = add(self.total, other.total);
        self.known += other.known;
        self.stored = self.stored.saturating_add(other.stored);
        self.priced += other.priced;
        // Two combinations can share a session, so their distinct counts cannot
        // be added; the folded series reports no count rather than a wrong one.
        self.sessions = None;
        for (index, point) in other.points {
            self.points.entry(index).or_default().merge(&point);
        }
    }

    fn finish(self, key: String, label: String, kind: &'static str) -> dto::TurnSeries {
        dto::TurnSeries {
            key,
            label,
            model: self.model,
            effort: self.effort,
            kind,
            turns: self.turns,
            accepted_observations: self.accepted.max(0) as u64,
            observed_sessions: self.sessions,
            tokens: Category::from_sum(self.total, self.known, self.accepted),
            estimated_cost: EstimatedCost {
                known_subtotal: (self.priced > 0).then(|| self.stored.to_string()),
                complete: self.accepted > 0 && self.accepted == self.priced,
            },
            points: self
                .points
                .iter()
                .map(|(index, point)| point.finish(*index))
                .collect(),
        }
    }
}

/// A combination's own identity, so an unattributed model and an unrecorded
/// reasoning effort never merge into a named one.
fn key(model: Option<&str>, effort: Option<&str>) -> String {
    format!(
        "{}|effort:{}",
        model.map_or("unknown:".to_owned(), |value| format!("model:{value}")),
        effort.unwrap_or_default()
    )
}

fn label(model: Option<&str>, effort: Option<&str>) -> String {
    format!(
        "{} · {}",
        model.unwrap_or("Unknown model"),
        effort.map_or("reasoning unavailable", |value| value)
    )
}

pub(super) fn read(
    tx: &Transaction<'_>,
    start: Time,
    end: Time,
    bins: u32,
    interval: &str,
) -> Result<dto::TurnActivity, ReadError> {
    let bins = bins.clamp(1, MAX_BINS);
    let nanos = |time: Time| i128::from(time.seconds) * 1_000_000_000 + i128::from(time.nanos);
    let (first, width) = (nanos(start), (nanos(end) - nanos(start)).max(1));
    tx.create_scalar_function(
        "dashboard_turn_bin",
        1,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        move |ctx| {
            let observed: i64 = ctx.get(0)?;
            Ok(((i128::from(observed) - first) * i128::from(bins) - 1).div_euclid(width) as i64)
        },
    )
    .map_err(|_| ReadError::Storage)?;

    // One pass: fold observations into turns, then turns into combinations. An
    // observation whose record carries no turn identity stands alone rather than
    // silently merging with every other identity-less observation in a session.
    let sql = format!("WITH scoped AS (
        SELECT o.id,o.thread_id,o.model,o.effort,o.total,
               COALESCE(json_extract(o.normalized,'$.turn_id'),'#'||o.id) AS turn_key,
               json_extract(o.normalized,'$.turn_id') IS NULL AS anonymous,
               o.time_seconds*1000000000+o.time_nanos AS ns,v.amount,v.observation_id AS valued
        FROM observations o LEFT JOIN observation_valuations v ON v.observation_id=o.id WHERE {interval}
    ), per_turn AS (
        SELECT thread_id,MAX(model) AS model,MAX(effort) AS effort,MAX(anonymous) AS anonymous,
               dashboard_turn_bin(MIN(ns)) AS bin,COUNT(*) AS accepted,SUM(total) AS total,
               COUNT(total) AS known,estimated_cost_sum(amount) AS amount,COUNT(valued) AS priced
        FROM scoped GROUP BY thread_id,turn_key
    ) SELECT model,effort,bin,COUNT(*),SUM(accepted),SUM(total),SUM(known),
             estimated_cost_sum(amount),SUM(priced),SUM(anonymous)
      FROM per_turn GROUP BY model IS NULL,model,effort,bin");
    let mut statement = tx.prepare(&sql).map_err(|_| ReadError::Storage)?;
    let mut rows = statement
        .query(params![start.seconds, start.nanos, end.seconds, end.nanos])
        .map_err(|_| ReadError::Storage)?;

    let mut grouped: BTreeMap<String, Series> = BTreeMap::new();
    let mut total_turns = 0u64;
    let mut without_identity = 0u64;
    while let Some(row) = rows.next().map_err(|_| ReadError::Storage)? {
        let read = || -> rusqlite::Result<_> {
            let model: Option<String> = row.get(0)?;
            let effort: Option<String> = row.get(1)?;
            let index: i64 = row.get(2)?;
            let turns: i64 = row.get(3)?;
            let amount: Option<String> = row.get(7)?;
            Ok((
                model,
                effort,
                index.clamp(0, i64::from(bins) - 1) as u32,
                turns.max(0) as u64,
                Point {
                    turns: turns.max(0) as u64,
                    accepted: row.get(4)?,
                    total: row.get(5)?,
                    known: row.get(6)?,
                    stored: 0,
                    priced: row.get(8)?,
                },
                amount,
                row.get::<_, Option<i64>>(9)?.unwrap_or(0).max(0) as u64,
            ))
        };
        let (model, effort, index, turns, mut point, amount, anonymous) =
            read().map_err(|_| ReadError::Storage)?;
        point.stored = amount
            .map(|value| value.parse::<i128>())
            .transpose()
            .map_err(|_| ReadError::Storage)?
            .unwrap_or(0);
        total_turns += turns;
        without_identity += anonymous;
        let entry = grouped
            .entry(key(model.as_deref(), effort.as_deref()))
            .or_default();
        entry.model = model;
        entry.effort = effort;
        entry.turns += turns;
        entry.accepted += point.accepted;
        entry.total = add(entry.total, point.total);
        entry.known += point.known;
        entry.stored = entry.stored.saturating_add(point.stored);
        entry.priced += point.priced;
        entry.points.entry(index).or_default().merge(&point);
    }

    // Distinct sessions must be counted over the whole range: a per-bin count
    // would only ever see the sessions active inside one bin.
    let sql = format!("WITH scoped AS (
        SELECT o.thread_id,o.model,o.effort,COALESCE(json_extract(o.normalized,'$.turn_id'),'#'||o.id) AS turn_key
        FROM observations o WHERE {interval}
    ), per_turn AS (
        SELECT thread_id,MAX(model) AS model,MAX(effort) AS effort FROM scoped GROUP BY thread_id,turn_key
    ) SELECT model,effort,COUNT(DISTINCT thread_id) FROM per_turn GROUP BY model IS NULL,model,effort");
    let mut statement = tx.prepare(&sql).map_err(|_| ReadError::Storage)?;
    let mut rows = statement
        .query(params![start.seconds, start.nanos, end.seconds, end.nanos])
        .map_err(|_| ReadError::Storage)?;
    while let Some(row) = rows.next().map_err(|_| ReadError::Storage)? {
        let read = || -> rusqlite::Result<_> {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, i64>(2)?,
            ))
        };
        let (model, effort, sessions) = read().map_err(|_| ReadError::Storage)?;
        if let Some(entry) = grouped.get_mut(&key(model.as_deref(), effort.as_deref())) {
            entry.sessions = Some(sessions.max(0) as u64);
        }
    }

    let combinations = grouped.len() as u64;
    // Usage that attributes neither a model nor a reasoning effort keeps its own
    // row, exactly as in the cost table: folding it away would hide the gap.
    let unattributed = grouped.remove(&key(None, None));
    let mut ranked: Vec<(String, Series)> = grouped.into_iter().collect();
    // Turns first, because this panel counts turns; tokens break ties so two
    // equally busy combinations still order by how much work they carried.
    ranked.sort_by(|(left_key, left), (right_key, right)| {
        right
            .turns
            .cmp(&left.turns)
            .then_with(|| right.total.cmp(&left.total))
            .then_with(|| left_key.cmp(right_key))
    });
    let mut series = Vec::new();
    let mut other = Series::default();
    let mut folded = 0usize;
    for (index, (id, entry)) in ranked.into_iter().enumerate() {
        if index < MAX_SERIES {
            let label = label(entry.model.as_deref(), entry.effort.as_deref());
            series.push(entry.finish(id, label, "combination"));
        } else {
            folded += 1;
            other.merge(entry);
        }
    }
    if folded > 0 {
        other.model = None;
        other.effort = None;
        series.push(other.finish(
            "other:".to_owned(),
            format!("Other combinations ({folded})"),
            "other",
        ));
    }
    if let Some(entry) = unattributed {
        let label = label(None, None);
        series.push(entry.finish(key(None, None), label, "unattributed"));
    }
    Ok(dto::TurnActivity {
        start, end, bin_count: bins, total_turns, combinations,
        turns_without_identity: without_identity, series,
        coverage_note: "A turn is one exchange with the model, counted once where it first appears in this range, with all of its tokens and cost counted there too. Reasoning effort comes from the turn's own context; turns recorded before this application started tracking it report it as unavailable. A session can run turns of several combinations, so the folded remainder reports no session count rather than a wrong one.",
    })
}
