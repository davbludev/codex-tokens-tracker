//! Disjoint quota intervals with per-observation, version-aware hypothesis prices.
mod hypotheses;
use crate::{
    aggregates::{Category, Tokens},
    dashboard::{QuotaAnalysis, QuotaInterval},
    storage::{
        aggregates::{row_tokens, token_fields},
        weekly,
    },
    weekly::{decimal, ReadError, Sample, Time, Timeline},
};
use bigdecimal::BigDecimal;
use rusqlite::{params, Connection, Rows};
use std::collections::VecDeque;

const MAX_INTERVALS: usize = 512;

#[derive(Default)]
struct Totals {
    sums: [u128; 6],
    missing: [bool; 6],
    observed: bool,
    hypotheses: hypotheses::Hypotheses,
}
impl Totals {
    fn add(&mut self, tokens: Tokens) -> Result<(), ReadError> {
        self.observed = true;
        for (index, category) in [
            tokens.total_tokens,
            tokens.input_tokens,
            tokens.cached_input_tokens,
            tokens.cache_write_tokens,
            tokens.output_tokens,
            tokens.reasoning_tokens,
        ]
        .into_iter()
        .enumerate()
        {
            self.missing[index] |= !category.complete;
            if let Some(value) = category.known_tokens {
                self.sums[index] = self.sums[index]
                    .checked_add(value.parse::<u128>().map_err(|_| ReadError::Storage)?)
                    .ok_or(ReadError::Storage)?;
            }
        }
        Ok(())
    }
    fn finish(&self) -> Tokens {
        let category = |i: usize| Category {
            known_tokens: (self.observed && !self.missing[i]).then(|| self.sums[i].to_string()),
            complete: self.observed && !self.missing[i],
        };
        Tokens {
            total_tokens: category(0),
            input_tokens: category(1),
            cached_input_tokens: category(2),
            cache_write_tokens: category(3),
            output_tokens: category(4),
            reasoning_tokens: category(5),
        }
    }
}

pub(super) fn read(
    connection: &Connection,
    start: Time,
    now: Time,
) -> Result<QuotaAnalysis, ReadError> {
    // Merge one grouped usage stream with canonical quota observations. Never
    // interpolate quota or run a usage-prefix query for every quota sample.
    // Immutable valuation version wins. Otherwise apply exactly the existing
    // effective-time / explicit first-price backfill rules, never latest price.
    let sql = format!("SELECT o.time_seconds,o.time_nanos,COUNT(*),{},o.normalized,p.configuration FROM observations o
        LEFT JOIN observation_valuations v ON v.observation_id=o.id
        LEFT JOIN model_price_versions p ON p.id=COALESCE(v.version_id,
          (SELECT id FROM model_price_versions WHERE model=o.model AND (effective_seconds,effective_nanos)<=(o.time_seconds,o.time_nanos) ORDER BY effective_seconds DESC,effective_nanos DESC LIMIT 1),
          (SELECT version_id FROM model_price_backfills WHERE model=o.model),
          (SELECT id FROM model_price_versions WHERE model=o.model AND backfill_before=1 ORDER BY effective_seconds,effective_nanos LIMIT 1))
        WHERE o.accepted=1 AND (o.time_seconds,o.time_nanos)>(?1,?2) AND (o.time_seconds,o.time_nanos)<=(?3,?4)
        GROUP BY o.id ORDER BY o.time_seconds,o.time_nanos,o.id", token_fields());
    let mut statement = connection.prepare(&sql).map_err(|_| ReadError::Storage)?;
    let mut rows = statement
        .query(params![start.seconds, start.nanos, now.seconds, now.nanos])
        .map_err(|_| ReadError::Storage)?;
    let mut next = next_tokens(&mut rows)?;
    let mut timeline = Timeline::new(now, None, 1);
    let mut anchor: Option<Sample> = None;
    let mut segment = None;
    let mut totals = Totals::default();
    let mut intervals = VecDeque::new();
    let mut total_intervals = 0;
    weekly::scan(connection, now, &mut timeline, |timeline, time| {
        let current_segment = timeline.baseline.as_ref().map(|sample| sample.time);
        let valid = time >= start
            && !timeline.ambiguous
            && timeline.latest.as_ref().is_some_and(|s| s.time == time);
        let continues = valid && segment == current_segment && anchor.is_some();
        while next
            .as_ref()
            .is_some_and(|(observed, _, _, _)| *observed <= time)
        {
            let (_, tokens, usage, rates) = next.take().unwrap();
            if continues {
                totals.add(tokens)?;
                totals.hypotheses.add(&usage, rates.as_ref());
            }
            next = next_tokens(&mut rows)?;
        }
        if !continues {
            totals = Totals::default();
            anchor = if valid { timeline.latest.clone() } else { None };
            segment = current_segment;
            return Ok(());
        }
        let first = anchor.as_ref().unwrap();
        let last = timeline.latest.as_ref().unwrap();
        let consumed = &last.used - &first.used;
        if consumed >= BigDecimal::from(1) {
            intervals.push_back(QuotaInterval {
                start: first.time,
                end: last.time,
                consumed_percentage_points: decimal(&consumed),
                tokens: totals.finish(),
                hypotheses: totals.hypotheses.finish(),
            });
            total_intervals += 1;
            if intervals.len() > MAX_INTERVALS {
                intervals.pop_front();
            }
            anchor = Some(last.clone());
            totals = Totals::default();
        }
        Ok(())
    })?;
    Ok(QuotaAnalysis {
        intervals: intervals.into_iter().collect(),
        total_intervals,
    })
}

type UsageRow = (
    Time,
    Tokens,
    crate::adapter::Tokens,
    Option<crate::pricing::Rates>,
);
fn next_tokens(rows: &mut Rows<'_>) -> Result<Option<UsageRow>, ReadError> {
    let Some(row) = rows.next().map_err(|_| ReadError::Storage)? else {
        return Ok(None);
    };
    let encoded: String = row.get(15).map_err(|_| ReadError::Storage)?;
    let usage: crate::adapter::Usage =
        serde_json::from_str(&encoded).map_err(|_| ReadError::Storage)?;
    let configuration: Option<String> = row.get(16).map_err(|_| ReadError::Storage)?;
    let rates = configuration
        .map(|encoded| {
            serde_json::from_str::<crate::pricing::PriceInput>(&encoded)
                .map_err(|_| ReadError::Storage)?
                .validate()
                .map_err(|_| ReadError::Storage)
        })
        .transpose()?;
    let read = || -> rusqlite::Result<_> {
        Ok((
            Time {
                seconds: row.get(0)?,
                nanos: row.get(1)?,
            },
            row_tokens(row, 3, row.get(2)?)?,
            usage.usage,
            rates,
        ))
    };
    read().map(Some).map_err(|_| ReadError::Storage)
}
