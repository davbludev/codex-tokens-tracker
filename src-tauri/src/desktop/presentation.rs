//! Exact weekly observation values prepared for the native tray.
use crate::weekly;

pub(crate) struct Summary {
    pub tooltip: String,
    pub rows: Vec<String>,
}

impl Summary {
    pub(crate) fn from_weekly(weekly: &weekly::Response) -> Self {
        let observation = weekly
            .current_cycle
            .as_ref()
            .map(|cycle| &cycle.last_observation);
        let percent = observation.map_or_else(
            || "Unavailable".into(),
            |observation| format!("{}%", observation.used_percent),
        );
        let (reason, short_reason) = weekly
            .overall
            .unavailable_reason
            .as_ref()
            .map(reason_text)
            .unwrap_or(("Unavailable", "unavailable"));
        let cost = weekly.overall.estimated_cost.as_ref().map_or_else(
            || format!("Unavailable — {reason}"),
            |cost| money(Some(cost)),
        );
        let ratio = if weekly.overall.unavailable_reason.is_some() {
            reason.into()
        } else {
            weekly
                .overall
                .effective_usd_per_percent
                .as_ref()
                .map_or_else(|| "Unavailable".into(), |value| format!("${value}"))
        };
        let tooltip_cost = match weekly.overall.estimated_cost.as_ref() {
            Some(cost) if !cost.complete => "incomplete".into(),
            cost => money(cost),
        };
        let tooltip_ratio = if weekly.overall.unavailable_reason.is_some() {
            short_reason
        } else {
            &ratio
        };
        let newer = money(weekly.unmatched_cost.as_ref());
        let (observed, tooltip_observed) = observation
            .map(|observation| utc(observation.time))
            .unwrap_or_else(|| ("Unavailable".into(), "Unavailable".into()));
        // Bound each value so Windows keeps the observation timestamp and scope visible.
        // The menu retains all digits when a tooltip value needs an ellipsis.
        let tooltip_percent = observation.map_or_else(
            || "Unavailable".into(),
            |observation| format!("{}%", compact(&observation.used_percent, 10)),
        );
        let tooltip_cost = compact(&tooltip_cost, 18);
        let tooltip_ratio = compact(tooltip_ratio, 18);
        Self {
            tooltip: format!("Weekly {tooltip_percent} · local/partial\nCost {tooltip_cost}\n~USD/% {tooltip_ratio}\nObserved {tooltip_observed}"),
            rows: vec![
                format!("Weekly usage: {}", observation.map_or_else(|| "Unavailable".into(), |_| format!("{percent} used (account-wide)"))),
                format!("Comparable estimated cost: {cost}"),
                format!("~USD / 1%: {ratio}"),
                format!("Newer unmatched estimated cost: {newer} (excluded from ratio)"),
                format!("Last observation: {observed}"),
                "Coverage: partial local observation interval".into(),
            ],
        }
    }

    pub(crate) fn unavailable() -> Self {
        Self {
            tooltip: "Weekly unavailable · local/partial\nCost unavailable\n~USD/% unavailable\nObservation unavailable".into(),
            rows: vec![
                "Weekly usage: Unavailable".into(),
                "Comparable estimated cost: Unavailable".into(),
                "~USD / 1%: Unavailable".into(),
                "Newer unmatched estimated cost: Unavailable (excluded from ratio)".into(),
                "Last observation: Unavailable".into(),
                "Coverage: partial local observation interval".into(),
            ],
        }
    }
}

fn compact(value: &str, limit: usize) -> String {
    if value.encode_utf16().count() <= limit {
        return value.into();
    }
    let mut remaining = limit - 1;
    let mut text: String = value
        .chars()
        .take_while(|character| {
            if character.len_utf16() > remaining {
                return false;
            }
            remaining -= character.len_utf16();
            true
        })
        .collect();
    text.push('…');
    text
}

fn money(cost: Option<&weekly::Cost>) -> String {
    let amount = cost
        .and_then(|cost| cost.known_subtotal.as_deref())
        .map_or_else(
            || "Unavailable".into(),
            |value| {
                let padded = format!("{value:0>13}");
                let (whole, fraction) = padded.split_at(padded.len() - 12);
                let fraction = fraction.trim_end_matches('0');
                if fraction.is_empty() {
                    format!("${whole}")
                } else {
                    format!("${whole}.{fraction}")
                }
            },
        );
    if cost.is_some_and(|cost| !cost.complete) {
        format!("{amount} (incomplete known subtotal)")
    } else {
        amount
    }
}

fn reason_text(reason: &weekly::Unavailable) -> (&'static str, &'static str) {
    match reason {
        weekly::Unavailable::InsufficientObservations => {
            ("Insufficient comparable observations", "insufficient")
        }
        weekly::Unavailable::AmbiguousObservation => ("Ambiguous observation", "ambiguous"),
        weekly::Unavailable::BelowOnePercentagePoint => {
            ("Less than 1 percentage point observed", "<1 pp")
        }
        weekly::Unavailable::UnpricedUsage => ("Unpriced usage — estimate unavailable", "unpriced"),
    }
}

fn utc(time: weekly::Time) -> (String, String) {
    match time::OffsetDateTime::from_unix_timestamp(time.seconds) {
        Ok(value) => {
            let seconds = format!(
                "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                value.year(),
                u8::from(value.month()),
                value.day(),
                value.hour(),
                value.minute(),
                value.second()
            );
            (
                format!("{seconds}.{:09} UTC", time.nanos),
                format!("{seconds}Z"),
            )
        }
        Err(_) => (
            format!("Unix {}.{:09} UTC", time.seconds, time.nanos),
            format!("Unix {} UTC", time.seconds),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response() -> weekly::Response {
        let start = weekly::Time {
            seconds: 1_700_000_000,
            nanos: 0,
        };
        let end = weekly::Time {
            seconds: 1_700_000_060,
            nanos: 123_456_789,
        };
        let observation = weekly::Observation {
            time: end,
            used_percent: "42.25".into(),
            remaining_percent: "57.75".into(),
            resets_at: None,
        };
        weekly::Response {
            evaluated_at: weekly::Time {
                seconds: 1_700_001_060,
                nanos: 0,
            },
            current_cycle: Some(weekly::Cycle {
                key: start.key(),
                first_observation: observation.clone(),
                last_observation: observation,
                detected_reset: false,
                has_ambiguous_observations: false,
                full_cycle_cost_known: false,
            }),
            observation_age_seconds: Some(999),
            overall: weekly::Estimate {
                start: Some(start),
                end: Some(end),
                consumed_percentage_points: Some("2".into()),
                estimated_cost: Some(weekly::Cost {
                    known_subtotal: Some("1234500000000".into()),
                    complete: true,
                    accepted_observations: 2,
                }),
                effective_usd_per_percent: Some("0.617250000000".into()),
                estimated_full_week_usd: Some("61.725000000000".into()),
                unavailable_reason: None,
            },
            recent: weekly::Estimate::unavailable(weekly::Unavailable::InsufficientObservations),
            unmatched_cost: Some(weekly::Cost {
                known_subtotal: Some("750000000000".into()),
                complete: true,
                accepted_observations: 1,
            }),
            unmatched_cost_start: Some(end),
            history: Vec::new(),
            next_cursor: None,
            excluded_samples: 0,
            session_weekly_percentage_impact: None,
            coverage_note: "Observed local estimated cost",
        }
    }

    #[test]
    fn priced_summary_keeps_comparable_and_newer_costs_separate_and_dates_observation() {
        let summary = Summary::from_weekly(&response());
        assert_eq!(
            summary.rows,
            vec![
                "Weekly usage: 42.25% used (account-wide)",
                "Comparable estimated cost: $1.2345",
                "~USD / 1%: $0.617250000000",
                "Newer unmatched estimated cost: $0.75 (excluded from ratio)",
                "Last observation: 2023-11-14 22:14:20.123456789 UTC",
                "Coverage: partial local observation interval",
            ]
        );
        assert!(summary.tooltip.contains("42.25%"));
        assert!(summary.tooltip.contains("local/partial"));
        assert!(summary.tooltip.contains("2023-11-14 22:14:20Z"));
        assert!(!summary.tooltip.contains("2023-11-14 22:31"));
    }

    #[test]
    fn unavailable_summaries_explain_limits_without_inventing_zero() {
        for (reason, expected) in [
            (
                weekly::Unavailable::InsufficientObservations,
                "Insufficient comparable observations",
            ),
            (
                weekly::Unavailable::AmbiguousObservation,
                "Ambiguous observation",
            ),
            (
                weekly::Unavailable::BelowOnePercentagePoint,
                "Less than 1 percentage point observed",
            ),
            (
                weekly::Unavailable::UnpricedUsage,
                "Unpriced usage — estimate unavailable",
            ),
        ] {
            let mut weekly = response();
            if reason == weekly::Unavailable::UnpricedUsage {
                weekly.overall.estimated_cost.as_mut().unwrap().complete = false;
                weekly.overall.effective_usd_per_percent = None;
                weekly.overall.unavailable_reason = Some(reason);
                weekly.unmatched_cost.as_mut().unwrap().complete = false;
                weekly.unmatched_cost.as_mut().unwrap().known_subtotal = None;
            } else {
                weekly.overall = weekly::Estimate::unavailable(reason);
            }
            let summary = Summary::from_weekly(&weekly);
            assert!(summary.rows[2].contains(expected), "{}", summary.rows[2]);
            assert!(!summary.rows[2].contains("$0"));
            if weekly.overall.estimated_cost.is_some() {
                assert_eq!(
                    summary.rows[1],
                    "Comparable estimated cost: $1.2345 (incomplete known subtotal)"
                );
                assert!(summary.rows[3].contains("Unavailable (incomplete known subtotal)"));
                assert!(summary.tooltip.contains("incomplete"));
                assert!(summary.tooltip.contains("unpriced"));
            } else {
                assert!(summary.rows[1].contains(expected));
            }
        }
        let mut missing = response();
        missing.current_cycle = None;
        missing.overall =
            weekly::Estimate::unavailable(weekly::Unavailable::InsufficientObservations);
        missing.unmatched_cost = None;
        let summary = Summary::from_weekly(&missing);
        assert_eq!(summary.rows.len(), Summary::unavailable().rows.len());
        assert_eq!(summary.rows[0], "Weekly usage: Unavailable");
        assert_eq!(summary.rows[4], "Last observation: Unavailable");
        assert!(!summary.tooltip.contains("UnavailableZ"));
        assert!(!summary.tooltip.contains("Unavailable%"));
    }

    #[test]
    fn large_exact_values_remain_in_menu_while_tooltip_fits_windows_budget() {
        let mut weekly = response();
        weekly
            .overall
            .estimated_cost
            .as_mut()
            .unwrap()
            .known_subtotal =
            Some(concat!("900719925474099312345678901234567890", "123456789012").into());
        weekly.overall.effective_usd_per_percent =
            Some("450359962737049656172839450617283945.061728394506".into());
        weekly
            .current_cycle
            .as_mut()
            .unwrap()
            .last_observation
            .used_percent = "42.250000000000000000000000000000000000000000000000000001".into();
        let summary = Summary::from_weekly(&weekly);
        assert_eq!(
            summary.rows[1],
            "Comparable estimated cost: $900719925474099312345678901234567890.123456789012"
        );
        assert_eq!(
            summary.rows[2],
            "~USD / 1%: $450359962737049656172839450617283945.061728394506"
        );
        assert!(
            summary.rows[0].contains("42.250000000000000000000000000000000000000000000000000001%")
        );
        assert!(
            summary.tooltip.encode_utf16().count() <= 127,
            "{}",
            summary.tooltip
        );
        assert!(summary.tooltip.contains('…'));
        assert!(summary.tooltip.contains("2023-11-14 22:14:20Z"));
        assert!(summary.tooltip.contains("local/partial"));
        assert!(Summary::unavailable().tooltip.encode_utf16().count() <= 127);
    }
}
