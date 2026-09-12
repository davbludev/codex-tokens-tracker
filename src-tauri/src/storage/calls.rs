use super::{weekly, Store};
use crate::{
    adapter::{Tokens, Usage},
    aggregates::{Category, EstimatedCost},
    calls::{self, BilledCategory, Categories},
    pricing::PriceInput,
    weekly::{ReadError, Time},
};
use rusqlite::{params, Connection, Row};
use std::path::Path;

pub(crate) fn open(path: &Path) -> Result<Connection, ReadError> {
    let connection = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| ReadError::Storage)?;
    connection
        .busy_timeout(std::time::Duration::from_secs(3))
        .map_err(|_| ReadError::Storage)?;
    Ok(connection)
}

impl Store {
    pub fn read_calls(path: &Path, query: calls::Query) -> Result<calls::Page, ReadError> {
        Self {
            connection: open(path)?,
        }
        .calls(query)
    }
    pub(crate) fn calls(&mut self, query: calls::Query) -> Result<calls::Page, ReadError> {
        query.validate()?;
        let cursor: Option<calls::Cursor> = query
            .after
            .as_deref()
            .map(|text| {
                if text.len() > 4096 {
                    return Err(ReadError::InvalidQuery);
                }
                serde_json::from_str(text).map_err(|_| ReadError::InvalidQuery)
            })
            .transpose()?;
        if cursor.as_ref().is_some_and(|cursor| {
            cursor.window != query.window()
                || cursor.model != query.model
                || cursor.thread != query.thread
                || !query.window().contains(cursor.time)
                || cursor.id <= 0
        }) {
            return Err(ReadError::InvalidQuery);
        }
        weekly::register(&self.connection)?;
        let tx = self
            .connection
            .transaction()
            .map_err(|_| ReadError::Storage)?;
        let mut statement = tx.prepare("SELECT o.id,o.time_seconds,o.time_nanos,o.thread_id,o.response_id,o.model,o.effort,o.normalized,v.amount,v.version_id,p.configuration FROM observations o LEFT JOIN observation_valuations v ON v.observation_id=o.id LEFT JOIN model_price_versions p ON p.id=v.version_id WHERE o.accepted=1 AND (o.time_seconds,o.time_nanos)>(?1,?2) AND (o.time_seconds,o.time_nanos)<=(?3,?4) AND (?5 IS NULL OR o.model=?5) AND (?6 IS NULL OR o.thread_id=?6) ORDER BY o.time_seconds,o.time_nanos,o.id").map_err(|_| ReadError::Storage)?;
        let mut rows = statement
            .query(params![
                query.start.seconds,
                query.start.nanos,
                query.end.seconds,
                query.end.nanos,
                query.model,
                query.thread
            ])
            .map_err(|_| ReadError::Storage)?;
        let mut total_items = 0u64;
        let mut items = Vec::new();
        let mut totals = Totals::default();
        let limit = query.limit.unwrap_or(50) as usize;
        let mut more = false;
        while let Some(row) = rows.next().map_err(|_| ReadError::Storage)? {
            let call = read_call(row)?;
            total_items += 1;
            totals.add(&call)?;
            let id = call.id.parse::<i64>().map_err(|_| ReadError::Storage)?;
            if cursor
                .as_ref()
                .is_some_and(|cursor| (call.time, id) <= (cursor.time, cursor.id))
            {
                continue;
            }
            if items.len() < limit {
                items.push(call);
            } else {
                more = true;
            }
        }
        let next_cursor = if more {
            items
                .last()
                .map(|call| {
                    serde_json::to_string(&calls::Cursor {
                        window: query.window(),
                        model: query.model.clone(),
                        thread: query.thread.clone(),
                        time: call.time,
                        id: call.id.parse().unwrap(),
                    })
                    .map_err(|_| ReadError::Storage)
                })
                .transpose()?
        } else {
            None
        };
        Ok(calls::Page {
            start: query.start,
            end: query.end,
            items,
            total_items,
            summary: totals.finish(),
            next_cursor,
        })
    }
}

fn raw_tokens(tokens: &Tokens) -> crate::aggregates::Tokens {
    let category = |value: &crate::adapter::Counter| Category {
        known_tokens: value.value().map(|n| n.to_string()),
        complete: value.value().is_some(),
    };
    crate::aggregates::Tokens {
        total_tokens: category(&tokens.total_tokens),
        input_tokens: category(&tokens.input_tokens),
        cached_input_tokens: category(&tokens.cached_input_tokens),
        cache_write_tokens: category(&tokens.cache_write_input_tokens),
        output_tokens: category(&tokens.output_tokens),
        reasoning_tokens: category(&tokens.reasoning_output_tokens),
    }
}

fn read_call(row: &Row<'_>) -> Result<calls::Call, ReadError> {
    let read = || -> rusqlite::Result<_> {
        Ok((
            row.get::<_, i64>(0)?,
            Time {
                seconds: row.get(1)?,
                nanos: row.get(2)?,
            },
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, String>(7)?,
            row.get::<_, Option<String>>(8)?,
            row.get::<_, Option<i64>>(9)?,
            row.get::<_, Option<String>>(10)?,
        ))
    };
    let (
        id,
        time,
        thread_id,
        response_id,
        model,
        effort,
        normalized,
        amount,
        version,
        configuration,
    ) = read().map_err(|_| ReadError::Storage)?;
    let usage: Usage = serde_json::from_str(&normalized).map_err(|_| ReadError::Storage)?;
    let price: Option<PriceInput> = configuration
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(|_| ReadError::Storage)?;
    let rates = price
        .as_ref()
        .map(PriceInput::validate)
        .transpose()
        .map_err(|_| ReadError::Storage)?;
    let parts = rates
        .as_ref()
        .and_then(|rates| rates.breakdown(&usage.usage).ok());
    let quantities = rates
        .as_ref()
        .and_then(|rates| rates.quantities(&usage.usage).ok());
    let parts = parts.filter(|parts| {
        amount
            .as_deref()
            .and_then(|amount| amount.parse::<i128>().ok())
            == parts
                .iter()
                .try_fold(0i128, |sum, value| sum.checked_add(*value))
    });
    let fallback_input = usage
        .usage
        .input_tokens
        .value()
        .zip(usage.usage.cached_input_tokens.value())
        .and_then(|(input, cached)| {
            if usage.usage.cache_write_input_tokens.value() == Some(0) {
                input.checked_sub(cached)
            } else {
                None
            }
        });
    let raw = [
        fallback_input,
        usage.usage.cached_input_tokens.value(),
        usage.usage.cache_write_input_tokens.value(),
        usage.usage.output_tokens.value(),
    ];
    let category = |index: usize| {
        let count = quantities
            .as_ref()
            .map(|values| values[index])
            .or(raw[index]);
        BilledCategory {
            tokens: Category {
                known_tokens: count.map(|n| n.to_string()),
                complete: count.is_some(),
            },
            estimated_cost: EstimatedCost {
                known_subtotal: parts.as_ref().map(|values| values[index].to_string()),
                complete: parts.is_some(),
            },
        }
    };
    Ok(calls::Call {
        id: id.to_string(),
        time,
        thread_id,
        turn_id: usage.turn_id.filter(|value| value.len() <= 1024),
        response_id: response_id.filter(|value| value.len() <= 1024),
        model: model.filter(|value| value.len() <= 1024),
        effort: effort.filter(|value| value.len() <= 64),
        tokens: raw_tokens(&usage.usage),
        categories: Categories {
            input: category(0),
            cached_input: category(1),
            cache_writes: category(2),
            output: category(3),
        },
        estimated_cost: EstimatedCost {
            complete: amount.is_some(),
            known_subtotal: amount,
        },
        price_version_id: version.map(|id| id.to_string()),
        price,
        category_reason: parts
            .is_none()
            .then_some("No preserved price or usable category split for this invocation"),
    })
}

#[derive(Default)]
struct Sum {
    value: i128,
    known: u64,
    count: u64,
}
impl Sum {
    fn add(&mut self, value: Option<&str>) -> Result<(), ReadError> {
        self.count += 1;
        if let Some(value) = value {
            self.known += 1;
            self.value = self
                .value
                .checked_add(value.parse::<i128>().map_err(|_| ReadError::Storage)?)
                .ok_or(ReadError::Storage)?;
        }
        Ok(())
    }
    fn text(&self) -> Option<String> {
        (self.known > 0 || self.count == 0).then(|| self.value.to_string())
    }
    fn cost(&self) -> EstimatedCost {
        EstimatedCost {
            known_subtotal: self.text(),
            complete: self.known == self.count,
        }
    }
}
#[derive(Default)]
struct Totals {
    cost: Sum,
    quantities: [Sum; 4],
    amounts: [Sum; 4],
}
impl Totals {
    fn add(&mut self, call: &calls::Call) -> Result<(), ReadError> {
        self.cost
            .add(call.estimated_cost.known_subtotal.as_deref())?;
        for (index, category) in [
            &call.categories.input,
            &call.categories.cached_input,
            &call.categories.cache_writes,
            &call.categories.output,
        ]
        .into_iter()
        .enumerate()
        {
            self.quantities[index].add(category.tokens.known_tokens.as_deref())?;
            self.amounts[index].add(category.estimated_cost.known_subtotal.as_deref())?;
        }
        Ok(())
    }
    fn finish(self) -> calls::Summary {
        let category = |index: usize| BilledCategory {
            tokens: Category {
                known_tokens: self.quantities[index].text(),
                complete: self.quantities[index].known == self.quantities[index].count,
            },
            estimated_cost: self.amounts[index].cost(),
        };
        calls::Summary {
            estimated_cost: self.cost.cost(),
            categories: Categories {
                input: category(0),
                cached_input: category(1),
                cache_writes: category(2),
                output: category(3),
            },
        }
    }
}
