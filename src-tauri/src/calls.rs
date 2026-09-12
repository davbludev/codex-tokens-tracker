//! One accepted usage delta is one model invocation, never a whole conversation turn.
use crate::{
    aggregates,
    weekly::{ReadError, Time},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Window {
    pub start: Time,
    pub end: Time,
}
impl Window {
    pub fn validate(&self) -> Result<(), ReadError> {
        if self.start >= self.end
            || self.start.nanos >= 1_000_000_000
            || self.end.nanos >= 1_000_000_000
        {
            Err(ReadError::InvalidQuery)
        } else {
            Ok(())
        }
    }
    pub fn contains(&self, time: Time) -> bool {
        time > self.start && time <= self.end
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Query {
    pub start: Time,
    pub end: Time,
    pub model: Option<String>,
    pub thread: Option<String>,
    pub after: Option<String>,
    pub limit: Option<u32>,
}
impl Query {
    pub fn window(&self) -> Window {
        Window {
            start: self.start,
            end: self.end,
        }
    }
    pub fn validate(&self) -> Result<(), ReadError> {
        self.window().validate()?;
        if !(1..=50).contains(&self.limit.unwrap_or(50))
            || [&self.model, &self.thread].iter().any(|field| {
                field
                    .as_ref()
                    .is_some_and(|text| text.is_empty() || text.len() > 512)
            })
        {
            return Err(ReadError::InvalidQuery);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BilledCategory {
    pub tokens: aggregates::Category,
    pub estimated_cost: aggregates::EstimatedCost,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Categories {
    pub input: BilledCategory,
    pub cached_input: BilledCategory,
    pub cache_writes: BilledCategory,
    pub output: BilledCategory,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Call {
    pub id: String,
    pub time: Time,
    pub thread_id: String,
    pub turn_id: Option<String>,
    pub response_id: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub tokens: aggregates::Tokens,
    pub categories: Categories,
    pub estimated_cost: aggregates::EstimatedCost,
    pub price_version_id: Option<String>,
    pub price: Option<crate::pricing::PriceInput>,
    pub category_reason: Option<&'static str>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub categories: Categories,
    pub estimated_cost: aggregates::EstimatedCost,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub start: Time,
    pub end: Time,
    pub items: Vec<Call>,
    pub total_items: u64,
    pub summary: Summary,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Cursor {
    pub window: Window,
    pub model: Option<String>,
    pub thread: Option<String>,
    pub time: Time,
    pub id: i64,
}
