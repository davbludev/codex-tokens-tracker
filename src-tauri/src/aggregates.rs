//! Prepared accounting data. Categories are independent projections, not addends.
use serde::{Deserialize, Serialize};
pub mod session_detail;
pub mod session_list;

pub const MAX_PAGE_SIZE: u32 = 50;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageRequest {
    pub after: Option<String>,
    pub limit: u32,
}
impl PageRequest {
    pub fn validate(&self) -> Result<(), ReadError> {
        if self.limit == 0 || self.limit > MAX_PAGE_SIZE {
            return Err(ReadError::InvalidQuery);
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Query {
    Global,
    Sessions {
        page: PageRequest,
    },
    SessionList {
        query: session_list::Query,
    },
    Session {
        thread: String,
    },
    SessionModels {
        thread: String,
        page: PageRequest,
    },
    SessionTimeline {
        thread: String,
        #[serde(rename = "pointBudget")]
        point_budget: Option<u32>,
    },
    Children {
        thread: String,
        page: PageRequest,
    },
    Ancestors {
        thread: String,
        page: PageRequest,
    },
    Projects {
        page: PageRequest,
    },
    Models {
        page: PageRequest,
    },
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ReadError {
    HierarchyPending,
    InvalidQuery,
    Storage,
}

/// A subtotal can be useful even when some observations lack this category.
/// None means no known value, including a session with no accepted usage.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub known_tokens: Option<String>,
    pub complete: bool,
}
impl Category {
    pub(crate) fn from_sum(sum: Option<i64>, known: i64, accepted: i64) -> Self {
        Self {
            known_tokens: sum.map(|n| n.to_string()),
            complete: accepted > 0 && known == accepted,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tokens {
    pub total_tokens: Category,
    pub input_tokens: Category,
    pub cached_input_tokens: Category,
    pub cache_write_tokens: Category,
    pub output_tokens: Category,
    pub reasoning_tokens: Category,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Coverage {
    pub incomplete_sessions: u64,
    pub unavailable_sessions: u64,
    pub unresolved_usage: bool,
    pub unknown_model: bool,
    pub unattributed_project: bool,
    pub source_diagnostics: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EstimatedCost {
    /// Canonical integer in 10^-12 USD. None means no priced accepted usage.
    pub known_subtotal: Option<String>,
    pub complete: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub tokens: Tokens,
    pub estimated_cost: EstimatedCost,
    pub coverage: Coverage,
    pub observed_at: Option<String>,
    pub observed_sessions: u64,
    pub placeholders: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub thread_id: String,
    pub title: Option<String>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub duration_seconds: Option<u64>,
    pub first_observed_at: Option<String>,
    pub last_observed_at: Option<String>,
    pub placeholder: bool,
    pub parent_state: String,
    pub parent_thread_id: Option<String>,
    pub project: Attribution,
    pub direct: Summary,
    /// Unavailable while hierarchy reconciliation is pending. Direct stays usable.
    pub inclusive: Option<Summary>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Attribution {
    pub id: String,
    pub basis: String,
    pub value: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub attribution: Attribution,
    pub direct: Summary,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
    /// Count and direct usage cover the whole selection, not just this page.
    pub total_items: u64,
    pub direct: Summary,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum Data {
    Global(Summary),
    Sessions(Page<Session>),
    SessionList(session_list::Page),
    Session(Option<Session>),
    SessionModels(Option<session_detail::Models>),
    SessionTimeline(Option<session_detail::Timeline>),
    Projects(Page<Group>),
    Models(Page<Group>),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Response {
    pub hierarchy_pending: bool,
    pub hierarchy_revision: i64,
    /// Each request is a consistent database read; subsequent pages may see new usage.
    pub data: Data,
    pub coverage_note: &'static str,
}
