//! Bounded, project-grouped direct-session browsing. Dates refer to last accepted usage.
use super::{Attribution, ReadError, Summary, MAX_PAGE_SIZE};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Sort {
    #[default]
    Newest,
    Usd,
    Tokens,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Query {
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
    #[serde(default)]
    pub sort: Sort,
    pub search: Option<String>,
    pub project: Option<String>,
    pub model: Option<String>,
    pub from_seconds: Option<i64>,
    pub before_seconds: Option<i64>,
}
impl Query {
    pub fn validate(&self) -> Result<(), ReadError> {
        if self.limit == 0
            || self.limit > MAX_PAGE_SIZE
            || [&self.search, &self.project, &self.model]
                .iter()
                .any(|v| v.as_ref().is_some_and(|s| s.len() > 1024))
            || self
                .from_seconds
                .zip(self.before_seconds)
                .is_some_and(|(a, b)| a >= b)
        {
            return Err(ReadError::InvalidQuery);
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub thread_id: String,
    pub title: Option<String>,
    pub last_observed_at: Option<String>,
    pub duration_seconds: Option<u64>,
    pub project: Attribution,
    /// At most eight names. A separate count exposes truncation without unbounded IPC.
    pub models: Vec<String>,
    pub model_count: u64,
    pub unknown_model: bool,
    pub direct: Summary,
    pub direct_subagent_count: Option<u64>,
    pub weekly_percentage_impact: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub items: Vec<Row>,
    pub total_items: u64,
    pub offset: u32,
    pub next_offset: Option<u32>,
}
