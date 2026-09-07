//! Pure classification of a durable forward path. Only a cycle's suffix loses edges.
#[derive(Debug, PartialEq, Eq)]
pub enum ReadError {
    HierarchyPending,
    Storage,
}

pub fn effective_state(
    candidate_state: &str,
    ordinal: i64,
    cycle_start: Option<i64>,
    is_self: bool,
) -> &str {
    if cycle_start.is_some_and(|start| ordinal >= start) {
        if is_self {
            "self"
        } else {
            "cycle"
        }
    } else {
        candidate_state
    }
}
