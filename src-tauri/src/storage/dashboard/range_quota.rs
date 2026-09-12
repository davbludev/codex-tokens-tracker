//! Selected-window estimates never borrow quota endpoints from outside the window.
use crate::{
    dashboard::{BoundaryKind, RangeQuota},
    weekly::{self, Cost, Estimate, Sample, Time, Unavailable},
};

#[derive(Default)]
struct Segment {
    first: Option<Sample>,
    last: Option<Sample>,
    amount: i128,
    known: bool,
    accepted: u64,
    complete: bool,
}
impl Segment {
    fn push(&mut self, sample: Sample, cost: &Cost) -> Result<(), weekly::ReadError> {
        if self.first.is_none() {
            self.first = Some(sample.clone());
            self.complete = true;
        } else {
            self.amount = self
                .amount
                .checked_add(
                    cost.known_subtotal
                        .as_deref()
                        .unwrap_or("0")
                        .parse::<i128>()
                        .map_err(|_| weekly::ReadError::Storage)?,
                )
                .ok_or(weekly::ReadError::Storage)?;
            self.accepted = self
                .accepted
                .checked_add(cost.accepted_observations)
                .ok_or(weekly::ReadError::Storage)?;
            self.known |= cost.known_subtotal.is_some() && cost.accepted_observations > 0;
            self.complete &= cost.complete;
        }
        self.last = Some(sample);
        Ok(())
    }
    fn estimate(&self) -> Estimate {
        match (&self.first, &self.last) {
            (Some(first), Some(last)) if first.time < last.time => Estimate::matched(
                first,
                last,
                Cost {
                    known_subtotal: (self.known || self.accepted == 0)
                        .then(|| self.amount.to_string()),
                    complete: self.complete,
                    accepted_observations: self.accepted,
                },
            ),
            _ => Estimate::unavailable(Unavailable::InsufficientObservations),
        }
    }
}

pub(super) struct SelectedQuota {
    start: Time,
    recent_start: Time,
    current: Segment,
    recent: Segment,
    segments: Vec<Estimate>,
    count: u64,
    latest: Option<Sample>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_unpriced_usage_has_no_invented_zero_subtotal() {
        let time = |seconds| Time { seconds, nanos: 0 };
        let mut selected = SelectedQuota::new(time(0), time(4));
        for seconds in 1..=3 {
            let sample = Sample {
                time: time(seconds),
                used: bigdecimal::BigDecimal::from(10 + seconds),
                reset: None,
            };
            selected
                .push(
                    Some(&sample),
                    sample.time,
                    None,
                    &Cost {
                        known_subtotal: None,
                        complete: false,
                        accepted_observations: 1,
                    },
                )
                .unwrap();
        }
        let result = selected.finish(None);
        let estimate = &result.segments[0];
        assert_eq!(
            estimate.unavailable_reason,
            Some(Unavailable::UnpricedUsage)
        );
        assert!(estimate
            .estimated_cost
            .as_ref()
            .unwrap()
            .known_subtotal
            .is_none());
        assert!(estimate.effective_usd_per_percent.is_none());
    }
}
impl SelectedQuota {
    pub fn new(start: Time, end: Time) -> Self {
        Self {
            start,
            recent_start: start.max(Time {
                seconds: end.seconds.saturating_sub(900),
                nanos: end.nanos,
            }),
            current: Segment::default(),
            recent: Segment::default(),
            segments: Vec::new(),
            count: 0,
            latest: None,
        }
    }
    fn close(&mut self) {
        if self.current.first.is_some() {
            self.count += 1;
            // A bounded last-50 projection, matching weekly history's page bound.
            if self.segments.len() == 50 {
                self.segments.remove(0);
            }
            self.segments.push(self.current.estimate());
        }
        self.current = Segment::default();
        self.recent = Segment::default();
    }
    pub fn push(
        &mut self,
        sample: Option<&Sample>,
        time: Time,
        boundary: Option<BoundaryKind>,
        cost: &Cost,
    ) -> Result<(), weekly::ReadError> {
        if time <= self.start {
            return Ok(());
        }
        if boundary.is_some() {
            self.close();
        }
        if boundary == Some(BoundaryKind::AmbiguousObservation) {
            return Ok(());
        }
        if let Some(sample) = sample {
            self.current.push(sample.clone(), cost)?;
            if time >= self.recent_start {
                self.recent.push(sample.clone(), cost)?;
            }
            self.latest = Some(sample.clone());
        }
        Ok(())
    }
    pub fn last_time(&self) -> Option<Time> {
        self.latest.as_ref().map(|sample| sample.time)
    }
    pub fn finish(mut self, unmatched_cost: Option<Cost>) -> RangeQuota {
        let recent = self.recent.estimate();
        self.close();
        RangeQuota {
            latest: self.latest.map(|sample| weekly::Observation {
                time: sample.time,
                used_percent: weekly::decimal(&sample.used),
                remaining_percent: weekly::decimal(
                    &(bigdecimal::BigDecimal::from(100) - sample.used),
                ),
                resets_at: sample.reset,
            }),
            segments: self.segments,
            total_segments: self.count,
            recent,
            unmatched_cost,
        }
    }
}
