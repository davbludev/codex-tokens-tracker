//! Bounded chart projection; exact native values survive until the drawing layer.
use crate::{
    aggregates,
    weekly::{self, Cost, Estimate, Sample, Time, Timeline, Unavailable},
};
use bigdecimal::BigDecimal;
use serde::{Deserialize, Serialize};

pub const MAX_POINTS: u32 = 4096;

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Range {
    CurrentCycle,
    Last24Hours,
    Last7Days,
    Last30Days,
    All,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Query {
    pub range: Range,
    pub point_budget: Option<u32>,
    #[serde(default)]
    pub breakdown_metric: BreakdownMetric,
}
impl Query {
    pub fn validate(&self) -> Result<usize, weekly::ReadError> {
        let budget = self.point_budget.unwrap_or(MAX_POINTS);
        if !(8..=MAX_POINTS).contains(&budget) {
            return Err(weekly::ReadError::InvalidQuery);
        }
        Ok((budget / 8) as usize)
    }
    pub fn start(&self, now: Time, cycle: Option<Time>, earliest: Option<Time>) -> Time {
        let seconds = match self.range {
            Range::CurrentCycle => return cycle.unwrap_or(now),
            Range::All => return earliest.unwrap_or(now),
            Range::Last24Hours => 86400,
            Range::Last7Days => 7 * 86400,
            Range::Last30Days => 30 * 86400,
        };
        Time {
            seconds: now.seconds.saturating_sub(seconds),
            nanos: now.nanos,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Response {
    pub evaluated_at: Time,
    pub weekly: weekly::Response,
    pub global: aggregates::Summary,
    pub token_scope: &'static str,
    pub chart: Chart,
    pub local_usage: LocalUsage,
    pub breakdowns: Breakdowns,
    pub turn_activity: TurnActivity,
    pub quota_analysis: QuotaAnalysis,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaInterval {
    pub start: Time,
    pub end: Time,
    pub consumed_percentage_points: String,
    pub tokens: aggregates::Tokens,
    pub hypotheses: Vec<QuotaHypothesis>,
    pub categories: CategoryCosts,
}

/// Estimated token cost split by token category, each observation at its own
/// model's preserved price version. Exact integer trillionths of USD; every
/// amount is absent when the scope has no usable priced split.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryCosts {
    pub input: Option<String>,
    pub cached_input: Option<String>,
    pub cache_writes: Option<String>,
    pub output: Option<String>,
    pub reason: Option<&'static str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaHypothesis {
    /// Optional bits: cached input=1, cache writes=2, reasoning=4.
    pub mask: u8,
    pub writes_included: bool,
    pub tokens: Option<String>,
    /// Exact integer trillionths of USD; absent if any usage is unpriced.
    pub estimated_usd: Option<String>,
    pub token_reason: Option<&'static str>,
    pub price_reason: Option<&'static str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaAnalysis {
    pub intervals: Vec<QuotaInterval>,
    pub total_intervals: u64,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BreakdownMetric {
    #[default]
    Tokens,
    Cost,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummary {
    pub tokens: aggregates::Tokens,
    pub estimated_cost: aggregates::EstimatedCost,
    pub observed_sessions: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsagePoint {
    pub index: u32,
    pub start: Time,
    pub end: Time,
    #[serde(flatten)]
    pub summary: UsageSummary,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalUsage {
    pub start: Time,
    pub end: Time,
    pub bin_count: u32,
    pub summary: UsageSummary,
    pub points: Vec<UsagePoint>,
    pub untimed_observations: u64,
    pub coverage_note: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Breakdown {
    pub key: String,
    pub label: String,
    pub kind: &'static str,
    pub tokens: aggregates::Category,
    pub estimated_cost: aggregates::EstimatedCost,
}

/// One model's direct usage in the selected range, with its estimated token
/// cost split into input, cached input, cache-write and output amounts. The
/// four amounts add up exactly to `estimated_cost.known_subtotal`, because each
/// is recomputed from the same stored valuation's own price version.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCost {
    pub key: String,
    pub label: String,
    pub kind: &'static str,
    pub tokens: aggregates::Tokens,
    pub estimated_cost: aggregates::EstimatedCost,
    pub categories: CategoryCosts,
    pub accepted_observations: u64,
    /// Absent when folding made the distinct set unrecoverable.
    pub observed_sessions: Option<u64>,
}

/// One time bin of one model-and-reasoning combination. A turn contributes its
/// whole tokens and cost to the bin its first accepted observation fell in.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnPoint {
    pub index: u32,
    pub turns: u64,
    pub tokens: aggregates::Category,
    pub estimated_cost: aggregates::EstimatedCost,
}

/// One combination of a model and the reasoning effort its turns ran at.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnSeries {
    pub key: String,
    pub label: String,
    pub model: Option<String>,
    /// The source's own wording, never a normalized or invented level.
    pub effort: Option<String>,
    /// `combination`, the folded `other` remainder, or `unattributed`.
    pub kind: &'static str,
    pub turns: u64,
    pub accepted_observations: u64,
    /// Absent when folding made the distinct set unrecoverable.
    pub observed_sessions: Option<u64>,
    pub tokens: aggregates::Category,
    pub estimated_cost: aggregates::EstimatedCost,
    pub points: Vec<TurnPoint>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnActivity {
    pub start: Time,
    pub end: Time,
    pub bin_count: u32,
    pub total_turns: u64,
    /// Distinct combinations before the remainder folds into one series.
    pub combinations: u64,
    /// Turns standing for an observation whose record carried no turn identity.
    pub turns_without_identity: u64,
    pub series: Vec<TurnSeries>,
    pub coverage_note: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Breakdowns {
    pub metric: BreakdownMetric,
    pub models: Vec<Breakdown>,
    pub projects: Vec<Breakdown>,
    /// Ranked by estimated cost; remaining models fold into one `other:` row.
    pub model_costs: Vec<ModelCost>,
    /// The same split across every model in the range, including `other:`.
    pub category_totals: CategoryCosts,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Point {
    pub time: Time,
    pub segment_id: Option<String>,
    pub weekly_used_percent: Option<String>,
    pub cumulative_estimated_cost: Option<Cost>,
    pub effective_usd_per_percent: Option<String>,
    pub unavailable_reason: Option<Unavailable>,
    pub connect_from_previous: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BoundaryKind {
    ObservationStart,
    Reset,
    AmbiguousObservation,
    UnpricedUsage,
    PricedUsageResumed,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Boundary {
    pub bin_index: usize,
    pub first_time: Time,
    pub last_time: Time,
    pub count: u64,
    pub kinds: Vec<BoundaryKind>,
    /// Multiple boundaries cannot be represented as one continuous chart span.
    pub overloaded: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Chart {
    pub range: Range,
    pub start: Time,
    pub end: Time,
    pub bin_count: usize,
    pub returned_observation_count: usize,
    pub source_observation_count: u64,
    pub points: Vec<Point>,
    pub boundaries: Vec<Boundary>,
    pub coverage_note: &'static str,
}

/// Cost input covers only (previous canonical time, current canonical time].
/// Timeline owns quota interpretation; this state only aligns chart cost segments.
#[derive(Default)]
pub(crate) struct Projection {
    baseline: Option<Sample>,
    quota_baseline: Option<Time>,
    amount: i128,
    known: bool,
    accepted: u64,
    complete: bool,
}
impl Projection {
    pub fn push(
        &mut self,
        timeline: &Timeline,
        time: Time,
        interval: Cost,
    ) -> Result<(Point, Option<BoundaryKind>), weekly::ReadError> {
        if timeline.ambiguous {
            self.baseline = None;
            self.quota_baseline = None;
            return Ok((
                Point {
                    time,
                    segment_id: None,
                    weekly_used_percent: None,
                    cumulative_estimated_cost: None,
                    effective_usd_per_percent: None,
                    unavailable_reason: Some(Unavailable::AmbiguousObservation),
                    connect_from_previous: false,
                },
                Some(BoundaryKind::AmbiguousObservation),
            ));
        }
        let sample = timeline.latest.as_ref().ok_or(weekly::ReadError::Storage)?;
        let quota_baseline = timeline.baseline.as_ref().map(|value| value.time);
        let fresh_quota = self.quota_baseline != quota_baseline || self.baseline.is_none();
        let transition = !fresh_quota && self.complete != interval.complete;
        let mut boundary = None;
        if fresh_quota || transition {
            boundary = Some(if fresh_quota {
                if timeline.current.as_ref().is_some_and(|cycle| {
                    cycle.detected_reset && cycle.first_observation.time == time
                }) {
                    BoundaryKind::Reset
                } else {
                    BoundaryKind::ObservationStart
                }
            } else if interval.complete {
                BoundaryKind::PricedUsageResumed
            } else {
                BoundaryKind::UnpricedUsage
            });
            self.baseline = Some(sample.clone());
            self.amount = 0;
            self.known = false;
            self.accepted = 0;
            self.complete = fresh_quota || interval.complete;
        }
        self.quota_baseline = quota_baseline;
        // A new trustworthy segment begins at its observation, so preceding
        // cost cannot be attributed to consumption after that baseline.
        if !fresh_quota && !(transition && interval.complete) {
            let amount = interval
                .known_subtotal
                .as_deref()
                .unwrap_or("0")
                .parse::<i128>()
                .map_err(|_| weekly::ReadError::Storage)?;
            self.amount = self
                .amount
                .checked_add(amount)
                .ok_or(weekly::ReadError::Storage)?;
            self.known |= interval.known_subtotal.is_some() && interval.accepted_observations > 0;
            self.accepted = self
                .accepted
                .checked_add(interval.accepted_observations)
                .ok_or(weekly::ReadError::Storage)?;
            self.complete &= interval.complete;
        }
        let cost = Cost {
            known_subtotal: (self.known || self.accepted == 0).then(|| self.amount.to_string()),
            complete: self.complete,
            accepted_observations: self.accepted,
        };
        let baseline = self.baseline.as_ref().unwrap();
        let estimate = if !self.complete {
            Estimate::unavailable(Unavailable::UnpricedUsage)
        } else if baseline.time == time {
            Estimate::unavailable(Unavailable::InsufficientObservations)
        } else {
            Estimate::matched(baseline, sample, cost.clone())
        };
        Ok((
            Point {
                time,
                segment_id: Some(baseline.time.key()),
                weekly_used_percent: Some(weekly::decimal(&sample.used)),
                cumulative_estimated_cost: Some(cost),
                effective_usd_per_percent: estimate.effective_usd_per_percent,
                unavailable_reason: estimate.unavailable_reason,
                connect_from_previous: false,
            },
            boundary,
        ))
    }
}

#[derive(Default)]
struct Bin {
    first: Option<Point>,
    last: Option<Point>,
    extrema: [Option<(BigDecimal, Point)>; 6],
    boundary: Option<Boundary>,
}

pub(crate) struct Downsample {
    range: Range,
    start: Time,
    end: Time,
    bins: Vec<Bin>,
    source_count: u64,
}
impl Downsample {
    pub fn new(range: Range, start: Time, end: Time, count: usize) -> Self {
        Self {
            range,
            start,
            end,
            bins: (0..count).map(|_| Bin::default()).collect(),
            source_count: 0,
        }
    }
    fn index(&self, time: Time) -> usize {
        let nanos =
            |value: Time| i128::from(value.seconds) * 1_000_000_000 + i128::from(value.nanos);
        let width = (nanos(self.end) - nanos(self.start)).max(1);
        (((nanos(time) - nanos(self.start)).max(0) * self.bins.len() as i128 / width) as usize)
            .min(self.bins.len() - 1)
    }
    pub fn push(&mut self, point: Point, boundary: Option<BoundaryKind>) {
        if point.time < self.start || point.time > self.end {
            return;
        }
        self.source_count += 1;
        let index = self.index(point.time);
        let bin = &mut self.bins[index];
        if let Some(kind) = boundary {
            let summary = bin.boundary.get_or_insert_with(|| Boundary {
                bin_index: index,
                first_time: point.time,
                last_time: point.time,
                count: 0,
                kinds: Vec::new(),
                overloaded: false,
            });
            summary.last_time = point.time;
            summary.count += 1;
            summary.overloaded = summary.count > 1;
            if !summary.kinds.contains(&kind) {
                summary.kinds.push(kind);
            }
        }
        if bin.first.is_none() {
            bin.first = Some(point.clone());
        }
        let values = [
            point.weekly_used_percent.as_deref(),
            point
                .cumulative_estimated_cost
                .as_ref()
                .and_then(|cost| cost.known_subtotal.as_deref()),
            point.effective_usd_per_percent.as_deref(),
        ];
        for (series, value) in values.into_iter().enumerate() {
            if let Some(value) = value.and_then(|value| value.parse::<BigDecimal>().ok()) {
                for maximum in [false, true] {
                    let slot = &mut bin.extrema[series * 2 + usize::from(maximum)];
                    if slot
                        .as_ref()
                        .is_none_or(|(old, _)| if maximum { value > *old } else { value < *old })
                    {
                        *slot = Some((value.clone(), point.clone()));
                    }
                }
            }
        }
        bin.last = Some(point);
    }
    pub fn finish(self) -> Chart {
        let bin_count = self.bins.len();
        let mut points: Vec<Point> = Vec::new();
        let mut boundaries = Vec::new();
        let mut previous_overloaded = false;
        for bin in self.bins {
            let overloaded = bin
                .boundary
                .as_ref()
                .is_some_and(|boundary| boundary.overloaded);
            let mut retained: Vec<_> = bin
                .first
                .into_iter()
                .chain(bin.last)
                .chain(bin.extrema.into_iter().flatten().map(|(_, point)| point))
                .collect();
            retained.sort_by_key(|point| point.time);
            retained.dedup_by_key(|point| point.time);
            for (index, mut point) in retained.into_iter().enumerate() {
                point.connect_from_previous = !overloaded
                    && !(index == 0 && previous_overloaded)
                    && point.segment_id.is_some()
                    && points
                        .last()
                        .is_some_and(|previous| previous.segment_id == point.segment_id);
                points.push(point);
            }
            previous_overloaded = overloaded;
            boundaries.extend(bin.boundary);
        }
        Chart { range: self.range, start: self.start, end: self.end, bin_count, returned_observation_count: points.len(), source_observation_count: self.source_count, points, boundaries,
            coverage_note: "Since observation began. Cumulative estimated USD restarts at trustworthy segment boundaries. Actual observations only; no interpolation. Range selection clips history without changing summary baselines." }
    }
}
