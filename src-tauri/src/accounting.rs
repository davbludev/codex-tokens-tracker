//! Pure acceptance rules for the evidence-supported non-resetting modern stream.
use crate::adapter::Tokens;

/// Only the facts needed to compare an observation with confirmed anchors.
#[derive(Clone, Copy)]
pub struct ObservationFacts<'a> {
    pub usage: &'a Tokens,
    pub endpoint: &'a Tokens,
    pub time: (i64, u32),
    pub source: &'a str,
    pub generation: i64,
    pub offset: i64,
}

pub enum Acceptance {
    Accept { incomplete_opening: bool },
    Pending,
}

fn relative_order(
    left: &ObservationFacts<'_>,
    right: &ObservationFacts<'_>,
) -> Option<std::cmp::Ordering> {
    use std::cmp::Ordering;
    match left.time.cmp(&right.time) {
        Ordering::Equal if left.source == right.source && left.generation == right.generation => {
            Some(left.offset.cmp(&right.offset))
        }
        Ordering::Equal => {
            endpoint_order(left.endpoint, right.endpoint).filter(|order| *order != Ordering::Equal)
        }
        order => Some(order),
    }
}

/// Select from at most six indexed neighbors. Confirmed observations stay
/// immutable: a candidate must independently validate and bridge both sides.
pub fn assess_candidate(
    candidate: ObservationFacts<'_>,
    anchors: &[ObservationFacts<'_>],
    unknown_anchor: bool,
) -> Acceptance {
    use std::cmp::Ordering;
    if unknown_anchor || anchors.len() > 6 {
        return Acceptance::Pending;
    }
    let mut predecessor = None;
    let mut successor = None;
    for anchor in anchors {
        match relative_order(anchor, &candidate) {
            Some(Ordering::Less) => {
                if let Some(old) = predecessor {
                    match relative_order(old, anchor) {
                        Some(Ordering::Less) => predecessor = Some(anchor),
                        Some(Ordering::Greater) => (),
                        _ => return Acceptance::Pending,
                    }
                } else {
                    predecessor = Some(anchor);
                }
            }
            Some(Ordering::Greater) => {
                if let Some(old) = successor {
                    match relative_order(anchor, old) {
                        Some(Ordering::Less) => successor = Some(anchor),
                        Some(Ordering::Greater) => (),
                        _ => return Acceptance::Pending,
                    }
                } else {
                    successor = Some(anchor);
                }
            }
            _ => return Acceptance::Pending,
        }
    }
    let standalone = reconcile(candidate.usage, candidate.endpoint, None);
    let bridges_before = reconcile(
        candidate.usage,
        candidate.endpoint,
        predecessor.map(|p| p.endpoint),
    )
    .is_ok();
    let bridges_after = successor
        .is_none_or(|next| reconcile(next.usage, next.endpoint, Some(candidate.endpoint)).is_ok());
    if standalone.is_ok() && bridges_before && bridges_after {
        Acceptance::Accept {
            incomplete_opening: predecessor.is_none() && standalone == Ok(true),
        }
    } else {
        Acceptance::Pending
    }
}

/// Counter order is evidence only for the supported non-resetting stream.
pub fn endpoint_order(left: &Tokens, right: &Tokens) -> Option<std::cmp::Ordering> {
    let left = left.values()?;
    let right = right.values()?;
    if left == right {
        Some(std::cmp::Ordering::Equal)
    } else if (0..6).all(|i| left[i] <= right[i]) {
        Some(std::cmp::Ordering::Less)
    } else if (0..6).all(|i| left[i] >= right[i]) {
        Some(std::cmp::Ordering::Greater)
    } else {
        None
    }
}

/// Indexed lookup key for neighbors, not acceptance evidence. Every selected
/// neighbor must still satisfy all six category relationships.
pub fn endpoint_key(tokens: &Tokens) -> Option<String> {
    counter_key(tokens.values()?)
}

pub fn start_key(usage: &Tokens, endpoint: &Tokens) -> Option<String> {
    let delta = usage.values()?;
    let end = endpoint.values()?;
    let mut start = [0; 6];
    for i in 0..6 {
        start[i] = end[i].checked_sub(delta[i])?;
    }
    counter_key(start)
}

fn counter_key(values: [i64; 6]) -> Option<String> {
    if values.iter().any(|value| *value < 0) {
        return None;
    }
    Some(
        [5, 0, 1, 2, 3, 4]
            .iter()
            .map(|i| format!("{:016x}", values[*i]))
            .collect(),
    )
}

pub fn reconcile(
    usage: &Tokens,
    endpoint: &Tokens,
    previous: Option<&Tokens>,
) -> Result<bool, &'static str> {
    let delta = usage
        .values()
        .ok_or("Missing reconciliation category; usage unavailable")?;
    let end = endpoint
        .values()
        .ok_or("Missing endpoint category; usage unavailable")?;
    for values in [delta, end] {
        if values.iter().any(|n| *n < 0) {
            return Err("Negative token counter; unsupported stream");
        }
        if values[0].checked_add(values[3]) != Some(values[5])
            || values[1] > values[0]
            || values[4] > values[3]
        {
            return Err("Unsupported token category relationship");
        }
    }
    if let Some(previous) = previous {
        let start = previous.values().ok_or("Previous endpoint unavailable")?;
        for i in 0..6 {
            if end[i] < start[i] {
                return Err("Decreasing thread endpoint; unsupported stream");
            }
            if end[i] - start[i] != delta[i] {
                return Err("Unexplained thread gap; remaining source usage unavailable");
            }
        }
        Ok(false)
    } else {
        if (0..6).any(|i| end[i] < delta[i]) {
            return Err("Usage exceeds opening endpoint");
        }
        Ok(end != delta)
    }
}
