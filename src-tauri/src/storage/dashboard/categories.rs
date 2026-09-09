//! Estimated cost split by token category, shared by the quota intervals and
//! the per-model rows. Each observation is valued with its own model's
//! preserved price version and policies, so the four amounts add up exactly to
//! the estimated token cost of whatever scope they were accumulated over.
use crate::{
    adapter::Tokens,
    dashboard::CategoryCosts,
    pricing::{self, Rates},
};

#[derive(Clone, Default)]
pub(super) struct Categories {
    amounts: [i128; 4],
    reason: Option<&'static str>,
    observed: bool,
}

impl Categories {
    pub(super) fn add(&mut self, tokens: &Tokens, rates: Option<&Rates>) {
        self.observed = true;
        let Some(rates) = rates else {
            self.reason = Some("Unpriced usage: no applicable model price");
            return;
        };
        match rates.breakdown(tokens) {
            Ok(parts) => {
                for (sum, amount) in self.amounts.iter_mut().zip(parts) {
                    match sum.checked_add(amount) {
                        Some(value) => *sum = value,
                        None => self.reason = Some("Estimated amount exceeds supported range"),
                    }
                }
            }
            Err(pricing::Error::Overflow) => {
                self.reason = Some("Estimated amount exceeds supported range")
            }
            Err(pricing::Error::MissingCategories) => self.reason = Some("Missing token counters"),
            Err(pricing::Error::InvalidCategories) => {
                self.reason = Some("Token categories violate the configured subset bounds")
            }
            Err(_) => self.reason = Some("Unsupported token semantics for this price version"),
        }
    }

    /// Adds another scope's amounts, keeping the first blocking reason.
    pub(super) fn merge(&mut self, other: Categories) {
        self.observed |= other.observed;
        for (sum, amount) in self.amounts.iter_mut().zip(other.amounts) {
            match sum.checked_add(amount) {
                Some(value) => *sum = value,
                None => self.reason = Some("Estimated amount exceeds supported range"),
            }
        }
        self.reason = self.reason.or(other.reason);
    }

    /// Exact sum of the four amounts, or None once a reason blocks the split.
    pub(super) fn checked_total(&self) -> Option<i128> {
        if self.reason.is_some() {
            return None;
        }
        self.amounts
            .iter()
            .try_fold(0i128, |sum, amount| sum.checked_add(*amount))
    }

    /// Marks the split unusable with the caller's own wording, which then
    /// replaces the default "no observations" reason.
    pub(super) fn reject(&mut self, reason: &'static str) {
        self.observed = true;
        self.reason = Some(reason);
    }

    pub(super) fn finish(&self) -> CategoryCosts {
        let reason = if self.observed {
            self.reason
        } else {
            Some("No local usage observations")
        };
        let amount = |index: usize| reason.is_none().then(|| self.amounts[index].to_string());
        CategoryCosts {
            input: amount(0),
            cached_input: amount(1),
            cache_writes: amount(2),
            output: amount(3),
            reason,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        adapter::Counter,
        pricing::{CacheWritePolicy, PriceInput, ReasoningPolicy},
    };
    fn tokens() -> Tokens {
        serde_json::from_value(serde_json::json!({"input_tokens":100,"cached_input_tokens":20,"cache_write_input_tokens":10,"output_tokens":50,"reasoning_output_tokens":15,"total_tokens":150})).unwrap()
    }
    fn rates(writes: CacheWritePolicy, separate: bool) -> Rates {
        PriceInput {
            input: "2".into(),
            cached_input: "0.5".into(),
            cache_write: "3".into(),
            output: "4".into(),
            reasoning: separate.then(|| "6".into()),
            reasoning_policy: if separate {
                ReasoningPolicy::Separate
            } else {
                ReasoningPolicy::Included
            },
            cache_write_policy: writes,
        }
        .validate()
        .unwrap()
    }
    #[test]
    fn categories_follow_each_versions_policies_and_sum_across_observations() {
        let mut values = Categories::default();
        values.add(&tokens(), Some(&rates(CacheWritePolicy::Additional, false)));
        values.add(
            &tokens(),
            Some(&rates(CacheWritePolicy::IncludedInputDisjoint, true)),
        );
        let costs = values.finish();
        // (80 + 70) input, (20 + 20) cached, (10 + 10) writes, (200 + 140 + 90) output.
        assert_eq!(
            [
                costs.input.as_deref(),
                costs.cached_input.as_deref(),
                costs.cache_writes.as_deref(),
                costs.output.as_deref(),
                costs.reason,
            ],
            [
                Some("300000000"),
                Some("20000000"),
                Some("60000000"),
                Some("430000000"),
                None
            ]
        );
    }
    #[test]
    fn unpriced_missing_or_invalid_usage_makes_every_category_unavailable() {
        let mut values = Categories::default();
        values.add(&tokens(), Some(&rates(CacheWritePolicy::Additional, false)));
        values.add(&tokens(), None);
        let costs = values.finish();
        assert!(costs.input.is_none() && costs.output.is_none());
        assert_eq!(
            costs.reason,
            Some("Unpriced usage: no applicable model price")
        );
        let mut values = Categories::default();
        values.add(
            &Tokens {
                cache_write_input_tokens: Counter::Missing,
                ..tokens()
            },
            Some(&rates(CacheWritePolicy::Additional, false)),
        );
        assert_eq!(values.finish().reason, Some("Missing token counters"));
        let mut values = Categories::default();
        values.add(
            &Tokens {
                cached_input_tokens: Counter::Known(101),
                ..tokens()
            },
            Some(&rates(CacheWritePolicy::Additional, false)),
        );
        assert_eq!(
            values.finish().reason,
            Some("Token categories violate the configured subset bounds")
        );
        assert_eq!(
            Categories::default().finish().reason,
            Some("No local usage observations")
        );
    }
}
