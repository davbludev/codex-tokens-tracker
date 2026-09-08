use crate::{adapter::Tokens, dashboard::QuotaHypothesis, pricing::Rates};

#[derive(Default)]
pub(super) struct Hypotheses {
    tokens: [i128; 16],
    amounts: [i128; 16],
    token_reasons: [Option<&'static str>; 16],
    price_reasons: [Option<&'static str>; 16],
    observed: bool,
}

fn components(tokens: &Tokens, mask: u8, included: bool) -> Result<[i64; 5], &'static str> {
    let counter = |value: &crate::adapter::Counter| {
        value
            .value()
            .filter(|v| *v >= 0)
            .ok_or("Missing token counters")
    };
    let input = counter(&tokens.input_tokens)?;
    let cached = counter(&tokens.cached_input_tokens)?;
    let output = counter(&tokens.output_tokens)?;
    let reasoning = counter(&tokens.reasoning_output_tokens)?;
    let writes = if included || mask & 2 != 0 {
        counter(&tokens.cache_write_input_tokens)?
    } else {
        0
    };
    let input = input
        .checked_sub(cached)
        .and_then(|v| v.checked_sub(if included { writes } else { 0 }))
        .filter(|v| *v >= 0)
        .ok_or("Invalid input subset subtraction")?;
    let output = output
        .checked_sub(reasoning)
        .filter(|v| *v >= 0)
        .ok_or("Invalid output subset subtraction")?;
    Ok([
        input,
        if mask & 1 != 0 { cached } else { 0 },
        if mask & 2 != 0 { writes } else { 0 },
        output,
        if mask & 4 != 0 { reasoning } else { 0 },
    ])
}

impl Hypotheses {
    pub(super) fn add(&mut self, tokens: &Tokens, rates: Option<&Rates>) {
        self.observed = true;
        for index in 0..16 {
            match components(tokens, (index % 8) as u8, index >= 8) {
                Err(reason) => self.token_reasons[index] = Some(reason),
                Ok(parts) => {
                    let total: i128 = parts.iter().map(|v| i128::from(*v)).sum();
                    match self.tokens[index].checked_add(total) {
                        Some(value) => self.tokens[index] = value,
                        None => {
                            self.token_reasons[index] = Some("Token total exceeds supported range")
                        }
                    }
                    match rates {
                        None => {
                            self.price_reasons[index] =
                                Some("Unpriced usage: no applicable model price")
                        }
                        Some(rates) => match rates
                            .hypothesis_value(parts)
                            .ok()
                            .and_then(|amount| self.amounts[index].checked_add(amount))
                        {
                            Some(value) => self.amounts[index] = value,
                            None => {
                                self.price_reasons[index] =
                                    Some("Estimated amount exceeds supported range")
                            }
                        },
                    }
                }
            }
        }
    }
    pub(super) fn finish(&self) -> Vec<QuotaHypothesis> {
        (0..16)
            .map(|index| {
                let token_reason = if self.observed {
                    self.token_reasons[index]
                } else {
                    Some("No local usage observations")
                };
                let price_reason = token_reason.or(self.price_reasons[index]);
                QuotaHypothesis {
                    mask: (index % 8) as u8,
                    writes_included: index >= 8,
                    tokens: token_reason
                        .is_none()
                        .then(|| self.tokens[index].to_string()),
                    estimated_usd: price_reason
                        .is_none()
                        .then(|| self.amounts[index].to_string()),
                    token_reason,
                    price_reason,
                }
            })
            .collect()
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
        serde_json::from_value(serde_json::json!({"input_tokens":100,"cached_input_tokens":20,"cache_write_input_tokens":10,"output_tokens":50,"reasoning_output_tokens":15})).unwrap()
    }
    fn rates(separate: bool) -> Rates {
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
            cache_write_policy: CacheWritePolicy::Additional,
        }
        .validate()
        .unwrap()
    }
    #[test]
    fn eight_unique_compositions_both_write_interpretations_and_model_component_prices() {
        let mut values = Hypotheses::default();
        values.add(&tokens(), Some(&rates(true)));
        let rows = values.finish();
        // Baseline 80 input + 35 visible output; cached 20, writes 10, reasoning 15.
        let expected_tokens = [115, 135, 125, 145, 130, 150, 140, 160];
        let expected_usd_units = [300, 310, 330, 340, 390, 400, 420, 430];
        for mask in 0..8 {
            for included in [false, true] {
                let row = &rows[mask + if included { 8 } else { 0 }];
                assert_eq!(row.mask, mask as u8);
                assert_eq!(row.writes_included, included);
                assert_eq!(
                    row.tokens,
                    Some((expected_tokens[mask] - if included { 10 } else { 0 }).to_string())
                );
                assert_eq!(
                    row.estimated_usd,
                    Some(
                        ((expected_usd_units[mask] - if included { 20 } else { 0 }) * 1_000_000i64)
                            .to_string()
                    )
                );
            }
        }
        let mut fallback = Hypotheses::default();
        fallback.add(&tokens(), Some(&rates(false)));
        assert_eq!(
            fallback.finish()[7].estimated_usd.as_deref(),
            Some("400000000")
        );
    }
    #[test]
    fn missing_and_invalid_counters_never_cancel_across_observations() {
        for field in 0..5 {
            let mut usage = tokens();
            match field {
                0 => usage.input_tokens = Counter::Missing,
                1 => usage.cached_input_tokens = Counter::Null,
                2 => usage.cache_write_input_tokens = Counter::Missing,
                3 => usage.output_tokens = Counter::Missing,
                _ => usage.reasoning_output_tokens = Counter::Missing,
            };
            let mut values = Hypotheses::default();
            values.add(&usage, Some(&rates(true)));
            assert!(values.finish()[7].tokens.is_none());
        }
        for usage in [
            Tokens {
                cached_input_tokens: Counter::Known(101),
                ..tokens()
            },
            Tokens {
                reasoning_output_tokens: Counter::Known(51),
                ..tokens()
            },
        ] {
            let mut values = Hypotheses::default();
            values.add(&usage, Some(&rates(true)));
            values.add(&tokens(), Some(&rates(true)));
            assert!(values
                .finish()
                .iter()
                .all(|row| row.tokens.is_none() && row.estimated_usd.is_none()));
        }
        let mut values = Hypotheses::default();
        values.add(
            &Tokens {
                cache_write_input_tokens: Counter::Known(81),
                ..tokens()
            },
            Some(&rates(true)),
        );
        assert!(values.finish()[0].tokens.is_some());
        assert!(values.finish()[8].tokens.is_none());
    }
    #[test]
    fn unpriced_usage_suppresses_money_but_retains_tokens() {
        let mut values = Hypotheses::default();
        values.add(&tokens(), Some(&rates(true)));
        values.add(&tokens(), None);
        assert!(values.finish().iter().all(|row| row.tokens.is_some()
            && row.estimated_usd.is_none()
            && row.price_reason.is_some()));
        assert!(Hypotheses::default()
            .finish()
            .iter()
            .all(|row| row.tokens.is_none() && row.estimated_usd.is_none()));
    }
}
