//! Per-model estimated cost, split into input, cached input, cache-write and
//! output amounts.
//!
//! Only observations that already carry a durable valuation contribute to the
//! split, and each is recomputed from that valuation's own price version. The
//! four amounts therefore add up exactly to the model's known cost subtotal,
//! which is checked against the stored amounts before the split is published.
use crate::{
    aggregates::{Category, EstimatedCost, Tokens},
    dashboard::{self as dto, CategoryCosts},
    storage::{
        aggregates::{token_fields, TOKEN_CATEGORIES},
        dashboard::categories::Categories,
    },
    weekly::{ReadError, Time},
};
use rusqlite::{params, Transaction};
use std::collections::BTreeMap;

/// Named rows kept before the remainder folds into one `other:` row.
const MAX_MODELS: usize = 8;
const UNKNOWN: &str = "unknown:";
const OTHER: &str = "other:";

/// Raw SQL sums, kept unrounded so folding the remainder stays exact.
#[derive(Clone, Default)]
struct Row {
    sums: [Option<i64>; TOKEN_CATEGORIES],
    known: [i64; TOKEN_CATEGORIES],
    accepted: i64,
    priced: i64,
    /// None once a fold makes the distinct set unrecoverable.
    sessions: Option<u64>,
    stored: i128,
    split: Categories,
}

impl Row {
    fn merge(&mut self, other: Row) {
        for index in 0..TOKEN_CATEGORIES {
            self.sums[index] = match (self.sums[index], other.sums[index]) {
                (Some(a), Some(b)) => Some(a.saturating_add(b)),
                (value, None) | (None, value) => value,
            };
            self.known[index] += other.known[index];
        }
        self.accepted += other.accepted;
        self.priced += other.priced;
        // Two models can share a session, so their distinct counts cannot be
        // added; the folded row reports no count rather than a wrong one.
        self.sessions = None;
        self.stored = self.stored.saturating_add(other.stored);
        self.split.merge(other.split);
    }

    fn tokens(&self) -> Tokens {
        let category =
            |index: usize| Category::from_sum(self.sums[index], self.known[index], self.accepted);
        Tokens {
            total_tokens: category(0),
            input_tokens: category(1),
            cached_input_tokens: category(2),
            cache_write_tokens: category(3),
            output_tokens: category(4),
            reasoning_tokens: category(5),
        }
    }

    /// The known subtotal is the stored valuation sum; the split is published
    /// only when it reconciles with it exactly.
    fn finish(mut self, key: String, label: String, kind: &'static str) -> dto::ModelCost {
        if self.priced == 0 {
            self.split.reject(if self.accepted == 0 {
                "No local usage observations"
            } else {
                "Unpriced usage: no applicable model price"
            });
        } else if self.split.checked_total() != Some(self.stored) {
            self.split
                .reject("Category split does not reconcile with the stored valuations");
        }
        dto::ModelCost {
            tokens: self.tokens(),
            estimated_cost: EstimatedCost {
                known_subtotal: (self.priced > 0).then(|| self.stored.to_string()),
                complete: self.accepted > 0 && self.accepted == self.priced,
            },
            categories: self.split.finish(),
            accepted_observations: self.accepted.max(0) as u64,
            observed_sessions: self.sessions,
            key,
            label,
            kind,
        }
    }
}

pub(super) fn read(
    tx: &Transaction<'_>,
    start: Time,
    end: Time,
    interval: &str,
) -> Result<(Vec<dto::ModelCost>, CategoryCosts), ReadError> {
    let parameters = params![start.seconds, start.nanos, end.seconds, end.nanos];
    let mut rows: BTreeMap<String, Row> = BTreeMap::new();
    let key = "CASE WHEN o.model IS NULL THEN 'unknown:' ELSE 'model:'||o.model END";

    // One grouped pass for tokens and counts. `model_id` never abbreviates to
    // `id`, which SQLite would resolve to the observation's own primary key and
    // silently group every observation on its own. The model count is bounded by
    // the models a user has actually run, so ranking happens in Rust where the
    // remainder folds without needing a second ranking rule in SQL.
    let sql = format!("SELECT {key} AS model_id,COUNT(*),{},COUNT(v.observation_id),COUNT(DISTINCT o.thread_id) FROM observations o LEFT JOIN observation_valuations v ON v.observation_id=o.id WHERE {interval} GROUP BY model_id", token_fields());
    let mut statement = tx.prepare(&sql).map_err(|_| ReadError::Storage)?;
    let mut grouped = statement.query(parameters).map_err(|_| ReadError::Storage)?;
    while let Some(row) = grouped.next().map_err(|_| ReadError::Storage)? {
        let read = || -> rusqlite::Result<(String, Row)> {
            let mut entry = Row {
                accepted: row.get(1)?,
                ..Row::default()
            };
            for index in 0..TOKEN_CATEGORIES {
                entry.sums[index] = row.get(2 + index * 2)?;
                entry.known[index] = row.get(3 + index * 2)?;
            }
            entry.priced = row.get(2 + TOKEN_CATEGORIES * 2)?;
            entry.sessions = Some(row.get::<_, i64>(3 + TOKEN_CATEGORIES * 2)?.max(0) as u64);
            Ok((row.get(0)?, entry))
        };
        let (id, entry) = read().map_err(|_| ReadError::Storage)?;
        rows.insert(id, entry);
    }

    // Second pass over valued observations only: recompute each amount from the
    // very price version its stored valuation used.
    let sql = format!("SELECT {key} AS model_id,o.normalized,v.amount,p.configuration FROM observations o JOIN observation_valuations v ON v.observation_id=o.id JOIN model_price_versions p ON p.id=v.version_id WHERE {interval}");
    let mut statement = tx.prepare(&sql).map_err(|_| ReadError::Storage)?;
    let mut valued = statement.query(parameters).map_err(|_| ReadError::Storage)?;
    while let Some(row) = valued.next().map_err(|_| ReadError::Storage)? {
        let id: String = row.get(0).map_err(|_| ReadError::Storage)?;
        let encoded: String = row.get(1).map_err(|_| ReadError::Storage)?;
        let stored: String = row.get(2).map_err(|_| ReadError::Storage)?;
        let configuration: String = row.get(3).map_err(|_| ReadError::Storage)?;
        let usage: crate::adapter::Usage =
            serde_json::from_str(&encoded).map_err(|_| ReadError::Storage)?;
        let rates = serde_json::from_str::<crate::pricing::PriceInput>(&configuration)
            .map_err(|_| ReadError::Storage)?
            .validate()
            .map_err(|_| ReadError::Storage)?;
        let entry = rows.entry(id).or_default();
        entry.split.add(&usage.usage, Some(&rates));
        entry.stored = entry
            .stored
            .checked_add(stored.parse::<i128>().map_err(|_| ReadError::Storage)?)
            .ok_or(ReadError::Storage)?;
    }

    let mut totals = Row::default();
    let mut named: Vec<(String, Row)> = Vec::new();
    let mut unknown = None;
    for (id, row) in rows {
        if id == UNKNOWN {
            unknown = Some(row);
        } else {
            named.push((id, row));
        }
    }
    // Cost first, then total tokens, so a model whose usage is not priced yet
    // still ranks by the activity the user can see.
    named.sort_by(|(left_id, left), (right_id, right)| {
        (right.priced > 0, right.stored)
            .cmp(&(left.priced > 0, left.stored))
            .then_with(|| right.sums[0].cmp(&left.sums[0]))
            .then_with(|| left_id.cmp(right_id))
    });
    let mut costs: Vec<dto::ModelCost> = Vec::new();
    let mut other = Row::default();
    let mut folded = 0usize;
    for (index, (id, row)) in named.into_iter().enumerate() {
        totals.merge(row.clone());
        if index < MAX_MODELS {
            let label = id.strip_prefix("model:").unwrap_or(&id).to_owned();
            costs.push(row.finish(id, label, "model"));
        } else {
            folded += 1;
            other.merge(row);
        }
    }
    if folded > 0 {
        costs.push(other.finish(
            OTHER.to_owned(),
            format!("Other models ({folded})"),
            "other",
        ));
    }
    if let Some(row) = unknown {
        totals.merge(row.clone());
        costs.push(row.finish(UNKNOWN.to_owned(), "Unknown model".to_owned(), "unknown"));
    }
    let totals = totals.finish(String::new(), String::new(), "model");
    Ok((costs, totals.categories))
}
