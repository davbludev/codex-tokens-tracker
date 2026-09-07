//! Account-wide quota observations and their comparable local-cost intervals.
use bigdecimal::{num_bigint::BigInt, BigDecimal};
use serde::{Deserialize, Serialize};
use std::{collections::VecDeque, str::FromStr};

pub const MAX_HISTORY: u32 = 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Time {
    pub seconds: i64,
    pub nanos: u32,
}
impl Time {
    pub fn key(self) -> String {
        format!("{}:{:09}", self.seconds, self.nanos)
    }
    fn from_key(value: &str) -> Option<Self> {
        let (seconds, nanos) = value.split_once(':')?;
        let time = Self {
            seconds: seconds.parse().ok()?,
            nanos: nanos.parse().ok()?,
        };
        (time.nanos < 1_000_000_000 && time.key() == value).then_some(time)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Query {
    pub before: Option<String>,
    pub limit: u32,
}
impl Query {
    pub fn validate(&self) -> Result<Option<Time>, ReadError> {
        if self.limit == 0 || self.limit > MAX_HISTORY {
            return Err(ReadError::InvalidQuery);
        }
        self.before
            .as_deref()
            .map(|key| Time::from_key(key).ok_or(ReadError::InvalidQuery))
            .transpose()
    }
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ReadError {
    InvalidQuery,
    Storage,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Observation {
    pub time: Time,
    pub used_percent: String,
    pub remaining_percent: String,
    pub resets_at: Option<i64>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cycle {
    pub key: String,
    pub first_observation: Observation,
    pub last_observation: Observation,
    pub detected_reset: bool,
    pub has_ambiguous_observations: bool,
    pub full_cycle_cost_known: bool,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cost {
    /// Canonical integer trillionths of USD, never a floating-point subtotal.
    pub known_subtotal: Option<String>,
    pub complete: bool,
    pub accepted_observations: u64,
}
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Unavailable {
    InsufficientObservations,
    AmbiguousObservation,
    BelowOnePercentagePoint,
    UnpricedUsage,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Estimate {
    pub start: Option<Time>,
    pub end: Option<Time>,
    pub consumed_percentage_points: Option<String>,
    pub estimated_cost: Option<Cost>,
    /// USD, rounded directly from the exact rational to 12 decimal places, half-even.
    pub effective_usd_per_percent: Option<String>,
    pub estimated_full_week_usd: Option<String>,
    pub unavailable_reason: Option<Unavailable>,
}
impl Estimate {
    pub fn unavailable(reason: Unavailable) -> Self {
        Self {
            start: None,
            end: None,
            consumed_percentage_points: None,
            estimated_cost: None,
            effective_usd_per_percent: None,
            estimated_full_week_usd: None,
            unavailable_reason: Some(reason),
        }
    }
    pub fn matched(start: &Sample, end: &Sample, cost: Cost) -> Self {
        let consumed = &end.used - &start.used;
        let reason = if !cost.complete {
            Some(Unavailable::UnpricedUsage)
        } else if consumed < BigDecimal::from(1) {
            Some(Unavailable::BelowOnePercentagePoint)
        } else {
            None
        };
        let amount = cost
            .known_subtotal
            .as_deref()
            .map(|value| BigInt::from_str(value).expect("storage supplies checked cost"));
        Self {
            start: Some(start.time),
            end: Some(end.time),
            consumed_percentage_points: Some(decimal(&consumed)),
            effective_usd_per_percent: reason
                .is_none()
                .then(|| ratio(amount.as_ref().unwrap(), &consumed, 1)),
            estimated_full_week_usd: reason
                .is_none()
                .then(|| ratio(amount.as_ref().unwrap(), &consumed, 100)),
            estimated_cost: Some(cost),
            unavailable_reason: reason,
        }
    }
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Response {
    pub evaluated_at: Time,
    pub current_cycle: Option<Cycle>,
    pub observation_age_seconds: Option<u64>,
    pub overall: Estimate,
    pub recent: Estimate,
    pub unmatched_cost: Option<Cost>,
    pub unmatched_cost_start: Option<Time>,
    pub history: Vec<Cycle>,
    pub next_cursor: Option<String>,
    pub excluded_samples: u64,
    pub session_weekly_percentage_impact: Option<String>,
    pub coverage_note: &'static str,
}

pub fn decimal(value: &BigDecimal) -> String {
    value.normalized().to_string()
}

/// Bound untrusted decimal work before parsing exponents or allocating powers of ten.
pub fn percentage(encoded: &str) -> Option<BigDecimal> {
    if encoded.len() > 256 {
        return None;
    }
    if let Some((_, exponent)) = encoded.split_once(['e', 'E']) {
        let exponent: i32 = exponent.parse().ok()?;
        if !(-1024..=1024).contains(&exponent) {
            return None;
        }
    }
    let value = BigDecimal::from_str(encoded).ok()?;
    (value >= BigDecimal::from(0) && value <= BigDecimal::from(100)).then_some(value)
}

// Cost already has 12 fractional USD digits. Integer quotient/remainder avoids
// a rounded decimal division followed by a second rounding at a halfway boundary.
fn ratio(amount: &BigInt, consumed: &BigDecimal, multiplier: u32) -> String {
    let (mut denominator, scale) = consumed.as_bigint_and_exponent();
    let mut numerator = amount * multiplier;
    if scale >= 0 {
        numerator *= BigInt::from(10).pow(scale as u32);
    } else {
        denominator *= BigInt::from(10).pow((-scale) as u32);
    }
    let mut quotient = &numerator / &denominator;
    let twice_remainder = (&numerator % &denominator) * 2;
    if twice_remainder > denominator
        || (twice_remainder == denominator && &quotient % 2 != BigInt::from(0))
    {
        quotient += 1;
    }
    format!("{:.12}", BigDecimal::new(quotient, 12))
}

#[derive(Clone, Debug)]
pub struct Sample {
    pub time: Time,
    pub used: BigDecimal,
    pub reset: Option<i64>,
}
impl Sample {
    fn observation(&self) -> Observation {
        Observation {
            time: self.time,
            used_percent: decimal(&self.used),
            remaining_percent: decimal(&(BigDecimal::from(100) - &self.used)),
            resets_at: self.reset,
        }
    }
}

/// Streaming chronological reducer: one tie group, one comparable segment and a
/// bounded history page, regardless of the number of retained source samples.
pub struct Timeline {
    pub current: Option<Cycle>,
    pub baseline: Option<Sample>,
    pub latest: Option<Sample>,
    pub recent_start: Option<Sample>,
    pub ambiguous: bool,
    history: VecDeque<Cycle>,
    before: Option<Time>,
    limit: usize,
    recent_cutoff: Time,
    group: Option<Sample>,
    conflict: bool,
    reset_conflict: bool,
}
impl Timeline {
    pub fn new(now: Time, before: Option<Time>, limit: u32) -> Self {
        Self {
            current: None,
            baseline: None,
            latest: None,
            recent_start: None,
            ambiguous: false,
            history: VecDeque::new(),
            before,
            limit: limit as usize,
            recent_cutoff: Time {
                seconds: now.seconds.saturating_sub(900),
                nanos: now.nanos,
            },
            group: None,
            conflict: false,
            reset_conflict: false,
        }
    }
    pub fn push(&mut self, sample: Sample) {
        if self
            .group
            .as_ref()
            .is_some_and(|group| group.time != sample.time)
        {
            self.flush();
        }
        if let Some(group) = &mut self.group {
            self.conflict |= group.used != sample.used;
            self.reset_conflict |= group.reset != sample.reset;
        } else {
            self.group = Some(sample);
        }
    }
    fn retain(&mut self, cycle: Cycle) {
        if self
            .before
            .is_none_or(|before| cycle.first_observation.time < before)
        {
            self.history.push_back(cycle);
            if self.history.len() > self.limit + 1 {
                self.history.pop_front();
            }
        }
    }
    fn flush(&mut self) {
        let Some(mut sample) = self.group.take() else {
            return;
        };
        if self.conflict {
            self.baseline = None;
            self.recent_start = None;
            self.ambiguous = true;
            if let Some(cycle) = &mut self.current {
                cycle.has_ambiguous_observations = true;
            }
        } else {
            if self.reset_conflict {
                sample.reset = None;
            }
            let reset = !self.ambiguous
                && self
                    .latest
                    .as_ref()
                    .is_some_and(|last| sample.used < last.used);
            if reset || self.current.is_none() {
                if let Some(cycle) = self.current.take() {
                    self.retain(cycle);
                }
                self.current = Some(Cycle {
                    key: sample.time.key(),
                    first_observation: sample.observation(),
                    last_observation: sample.observation(),
                    detected_reset: reset,
                    has_ambiguous_observations: false,
                    full_cycle_cost_known: false,
                });
                self.baseline = None;
                self.recent_start = None;
            }
            if self.baseline.is_none() {
                self.baseline = Some(sample.clone());
            }
            if self.recent_start.is_none() && sample.time >= self.recent_cutoff {
                self.recent_start = Some(sample.clone());
            }
            self.current.as_mut().unwrap().last_observation = sample.observation();
            self.latest = Some(sample);
            self.ambiguous = false;
        }
        self.conflict = false;
        self.reset_conflict = false;
    }
    pub fn finish(&mut self) -> (Vec<Cycle>, Option<String>) {
        self.flush();
        // The current cycle is returned separately; history contains completed cycles.
        let more = self.history.len() > self.limit;
        if more {
            self.history.pop_front();
        }
        let page: Vec<_> = self.history.drain(..).rev().collect();
        let next = more.then(|| page.last().unwrap().key.clone());
        (page, next)
    }
}
