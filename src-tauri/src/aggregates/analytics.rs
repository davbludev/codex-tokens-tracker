//! Prepared project comparisons and model history; all usage is direct.
use super::{session_detail::ShareUnavailable, Attribution, EstimatedCost, Summary, Tokens};
use crate::weekly::{Time, Unavailable};
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AverageCost {
    /// Integer trillionths of USD per observed session, rounded half-up.
    pub amount: Option<String>,
    pub unavailable_reason: Option<&'static str>,
}

pub(crate) fn average(summary: &Summary) -> Result<AverageCost, super::ReadError> {
    let reason = if summary.observed_sessions == 0 {
        Some("noObservedSessions")
    } else if summary.coverage.unavailable_sessions > 0 {
        Some("sessionsWithoutAcceptedUsage")
    } else if !summary.estimated_cost.complete {
        Some("incompleteCost")
    } else {
        None
    };
    let amount = if reason.is_none() {
        let amount: i128 = summary
            .estimated_cost
            .known_subtotal
            .as_deref()
            .ok_or(super::ReadError::Storage)?
            .parse()
            .map_err(|_| super::ReadError::Storage)?;
        let count = i128::from(summary.observed_sessions);
        Some((amount / count + i128::from(amount % count >= (count + 1) / 2)).to_string())
    } else {
        None
    };
    Ok(AverageCost {
        amount,
        unavailable_reason: reason,
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Classification {
    pub proven_subagent: Summary,
    pub parent_classification_unavailable: Summary,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CycleUsage {
    pub label: &'static str,
    pub cycle_key: Option<String>,
    pub start: Option<Time>,
    pub end: Option<Time>,
    pub observation_age_seconds: Option<u64>,
    pub partial: bool,
    pub has_ambiguous_observations: bool,
    pub unavailable_reason: Option<Unavailable>,
    pub direct: Option<Summary>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectModels {
    pub items: Vec<Attribution>,
    pub total_items: u64,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub attribution: Attribution,
    pub direct: Summary,
    pub average_session_cost: AverageCost,
    pub models: ProjectModels,
    pub classification: Option<Classification>,
    pub current_cycle: CycleUsage,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Projects {
    pub evaluated_at: Time,
    pub items: Vec<Project>,
    pub total_items: u64,
    pub next_cursor: Option<String>,
    pub direct: Summary,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivePrice {
    pub version_id: i64,
    pub effective_at: Time,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    pub attribution: Attribution,
    pub direct: Summary,
    pub accepted_usage_events: u64,
    pub sessions_used: u64,
    pub cost_share: Option<String>,
    pub cost_share_unavailable_reason: Option<ShareUnavailable>,
    pub active_pricing_version: Option<ActivePrice>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Models {
    pub evaluated_at: Time,
    pub items: Vec<Model>,
    pub total_items: u64,
    pub next_cursor: Option<String>,
    pub direct: Summary,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryBin {
    pub index: u32,
    pub start: Time,
    pub end: Time,
    pub accepted_usage_events: u64,
    pub tokens: Tokens,
    pub estimated_cost: EstimatedCost,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct History {
    pub attribution: Attribution,
    pub start: Time,
    pub end: Time,
    pub point_budget: u32,
    pub bins: Vec<HistoryBin>,
    pub untimed_accepted_usage_events: u64,
    pub coverage_note: &'static str,
}
