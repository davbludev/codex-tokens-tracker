//! Direct-session detail contracts and bounded, exact history projection.
use super::{Attribution, Category, EstimatedCost, ReadError, Summary};
use crate::weekly::Time;
use bigdecimal::num_bigint::BigInt;
use serde::Serialize;

pub const DEFAULT_POINTS: u32 = 512;
pub const MAX_POINTS: u32 = 4096;

pub(crate) fn point_budget(value: Option<u32>) -> Result<usize, ReadError> {
    let value = value.unwrap_or(DEFAULT_POINTS);
    if !(8..=MAX_POINTS).contains(&value) {
        return Err(ReadError::InvalidQuery);
    }
    Ok(value as usize)
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ShareUnavailable {
    Incomplete,
    Unavailable,
    ZeroDenominator,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsage {
    pub attribution: Attribution,
    pub direct: Summary,
    /// Decimal percentage of the whole session's direct estimated cost.
    pub cost_share: Option<String>,
    pub cost_share_unavailable_reason: Option<ShareUnavailable>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Models {
    pub scope: &'static str,
    pub items: Vec<ModelUsage>,
    pub next_cursor: Option<String>,
    pub total_items: u64,
    /// Whole-session denominator, never the page subtotal.
    pub direct: Summary,
}

pub(crate) fn cost_share(
    row: &EstimatedCost,
    session: &EstimatedCost,
) -> Result<(Option<String>, Option<ShareUnavailable>), ReadError> {
    let (Some(row_value), Some(session_value)) = (&row.known_subtotal, &session.known_subtotal)
    else {
        return Ok((None, Some(ShareUnavailable::Unavailable)));
    };
    if !row.complete || !session.complete {
        return Ok((None, Some(ShareUnavailable::Incomplete)));
    }
    // BigInt avoids overflowing an otherwise valid i128 amount when scaling.
    let numerator = row_value
        .parse::<BigInt>()
        .map_err(|_| ReadError::Storage)?
        * 10_000;
    let denominator = session_value
        .parse::<BigInt>()
        .map_err(|_| ReadError::Storage)?;
    if denominator == BigInt::from(0) {
        return Ok((None, Some(ShareUnavailable::ZeroDenominator)));
    }
    let mut hundredths: BigInt = &numerator / &denominator;
    if (&numerator % &denominator) * 2 >= denominator {
        hundredths += 1;
    }
    Ok((
        Some(format!("{}.{:0>2}", &hundredths / 100, &hundredths % 100)),
        None,
    ))
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Point {
    pub time: Time,
    pub cumulative_total_tokens: Category,
    pub cumulative_estimated_cost: EstimatedCost,
    pub tokens_connect_from_previous: bool,
    pub cost_connect_from_previous: bool,
    #[serde(skip)]
    segments: [Option<u64>; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BoundaryKind {
    ObservationStart,
    MissingTokens,
    TokensResumed,
    UnpricedUsage,
    PricedUsageResumed,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Boundary {
    pub bin_index: usize,
    pub first_time: Time,
    pub last_time: Time,
    /// Number of timestamp groups with a boundary (not number of kinds).
    pub count: u64,
    pub kinds: Vec<BoundaryKind>,
    pub overloaded: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Timeline {
    pub scope: &'static str,
    pub time_source: &'static str,
    pub first_observed_at: Option<Time>,
    pub last_observed_at: Option<Time>,
    pub point_budget: usize,
    pub bin_count: usize,
    pub source_observation_count: u64,
    pub source_point_count: u64,
    pub returned_point_count: usize,
    pub untimed_observation_count: u64,
    /// Includes accepted untimed usage; chart points cannot include that usage.
    pub direct: Summary,
    pub points: Vec<Point>,
    pub boundaries: Vec<Boundary>,
    pub coverage_note: &'static str,
}

/// One SQL group containing all accepted observations at an identical timestamp.
pub(crate) struct UsageGroup {
    pub time: Time,
    pub accepted: u64,
    pub tokens: Option<i64>,
    pub tokens_known: u64,
    pub cost: Option<String>,
    pub priced: u64,
}

#[derive(Default)]
pub(crate) struct Projection {
    tokens: Option<i64>,
    cost: Option<i128>,
    accepted: u64,
    tokens_known: u64,
    priced: u64,
    previous_complete: Option<[bool; 2]>,
    segments: [u64; 2],
}
impl Projection {
    pub fn push(&mut self, group: UsageGroup) -> Result<(Point, Vec<BoundaryKind>), ReadError> {
        let complete = [
            group.tokens_known == group.accepted,
            group.priced == group.accepted,
        ];
        let mut boundaries = Vec::new();
        if self.previous_complete.is_none() {
            boundaries.push(BoundaryKind::ObservationStart);
        }
        for (series, available) in complete.into_iter().enumerate() {
            let previous = self.previous_complete.map(|value| value[series]);
            if previous != Some(available) {
                self.segments[series] += 1;
                let kind = match (series, available) {
                    (0, false) => BoundaryKind::MissingTokens,
                    (0, true) => BoundaryKind::TokensResumed,
                    (_, false) => BoundaryKind::UnpricedUsage,
                    (_, true) => BoundaryKind::PricedUsageResumed,
                };
                if previous.is_some() || !available {
                    boundaries.push(kind);
                }
            }
        }
        self.previous_complete = Some(complete);
        self.accepted = self
            .accepted
            .checked_add(group.accepted)
            .ok_or(ReadError::Storage)?;
        self.tokens_known = self
            .tokens_known
            .checked_add(group.tokens_known)
            .ok_or(ReadError::Storage)?;
        self.priced = self
            .priced
            .checked_add(group.priced)
            .ok_or(ReadError::Storage)?;
        if let Some(tokens) = group.tokens {
            self.tokens = Some(
                self.tokens
                    .unwrap_or(0)
                    .checked_add(tokens)
                    .ok_or(ReadError::Storage)?,
            );
        }
        if let Some(cost) = group.cost {
            self.cost = Some(
                self.cost
                    .unwrap_or(0)
                    .checked_add(cost.parse::<i128>().map_err(|_| ReadError::Storage)?)
                    .ok_or(ReadError::Storage)?,
            );
        }
        Ok((
            Point {
                time: group.time,
                cumulative_total_tokens: Category {
                    known_tokens: self.tokens.map(|value| value.to_string()),
                    complete: self.tokens_known == self.accepted,
                },
                cumulative_estimated_cost: EstimatedCost {
                    known_subtotal: self.cost.map(|value| value.to_string()),
                    complete: self.priced == self.accepted,
                },
                tokens_connect_from_previous: false,
                cost_connect_from_previous: false,
                segments: [
                    complete[0].then_some(self.segments[0]),
                    complete[1].then_some(self.segments[1]),
                ],
            },
            boundaries,
        ))
    }
}

#[derive(Default)]
struct Bin {
    first: Option<Point>,
    last: Option<Point>,
    boundary: Option<Boundary>,
}

/// Cumulative nonnegative series have their extrema at the endpoints. Retain
/// first/last per time bin, plus bounded gap summaries and series continuity IDs.
pub(crate) struct Downsample {
    start: Time,
    end: Time,
    bins: Vec<Bin>,
}
impl Downsample {
    pub fn new(start: Time, end: Time, budget: usize) -> Self {
        Self {
            start,
            end,
            bins: (0..budget / 2).map(|_| Bin::default()).collect(),
        }
    }
    pub fn push(&mut self, point: Point, kinds: Vec<BoundaryKind>) {
        let nanos = |time: Time| i128::from(time.seconds) * 1_000_000_000 + i128::from(time.nanos);
        let width = (nanos(self.end) - nanos(self.start)).max(1);
        let index = (((nanos(point.time) - nanos(self.start)) * self.bins.len() as i128 / width)
            as usize)
            .min(self.bins.len() - 1);
        let bin = &mut self.bins[index];
        if !kinds.is_empty() {
            let boundary = bin.boundary.get_or_insert_with(|| Boundary {
                bin_index: index,
                first_time: point.time,
                last_time: point.time,
                count: 0,
                kinds: Vec::new(),
                overloaded: false,
            });
            boundary.last_time = point.time;
            boundary.count += 1;
            boundary.overloaded = boundary.count > 1;
            for kind in kinds {
                if !boundary.kinds.contains(&kind) {
                    boundary.kinds.push(kind);
                }
            }
        }
        if bin.first.is_none() {
            bin.first = Some(point.clone());
        }
        bin.last = Some(point);
    }
    pub fn finish(self) -> (Vec<Point>, Vec<Boundary>) {
        let mut points: Vec<Point> = Vec::new();
        let mut boundaries = Vec::new();
        let mut previous_overloaded = false;
        for bin in self.bins {
            // Empty bins must not erase the prior retained bin's barrier.
            if bin.first.is_none() {
                continue;
            }
            let overloaded = bin.boundary.as_ref().is_some_and(|value| value.overloaded);
            let mut retained: Vec<_> = bin.first.into_iter().chain(bin.last).collect();
            retained.dedup_by_key(|point| point.time);
            for (index, mut point) in retained.into_iter().enumerate() {
                let connect = |series: usize| {
                    !overloaded
                        && !(index == 0 && previous_overloaded)
                        && point.segments[series].is_some()
                        && points.last().is_some_and(|previous| {
                            previous.segments[series] == point.segments[series]
                        })
                };
                point.tokens_connect_from_previous = connect(0);
                point.cost_connect_from_previous = connect(1);
                points.push(point);
            }
            previous_overloaded = overloaded;
            boundaries.extend(bin.boundary);
        }
        (points, boundaries)
    }
}
