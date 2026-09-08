//! Exact estimated USD rules. Rates are micro-USD per million tokens;
//! valuation amounts are integer trillionths (10^-12) of a USD.
use crate::adapter::Tokens;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("Enter a nonnegative decimal price with at most six fractional digits within the supported range")]
    InvalidRate { field: &'static str },
    #[error("A reasoning price is required only for separately priced reasoning")]
    ReasoningRate,
    #[error("Select a detected named model")]
    UnknownModel,
    #[error("Older usage can be covered only by the model's first price")]
    BackfillOnlyFirst,
    #[error("No older unpriced usage is available for this model")]
    BackfillUnavailable,
    #[error("The system clock must be later than this model's last price change")]
    Clock,
    #[error("Token categories are missing")]
    MissingCategories,
    #[error("Token categories violate the configured subset bounds")]
    InvalidCategories,
    #[error("Reasoning interpretation is unknown")]
    UnknownReasoning,
    #[error("Nonzero cache-write interpretation is unknown")]
    UnknownCacheWrite,
    #[error("Estimated token cost exceeds the supported exact range")]
    Overflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningPolicy {
    Unknown,
    Included,
    Separate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheWritePolicy {
    Unknown,
    IncludedInputDisjoint,
    Additional,
}

/// No client effective timestamp: the Store assigns it when saving.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PriceInput {
    pub input: String,
    pub cached_input: String,
    pub cache_write: String,
    pub output: String,
    pub reasoning: Option<String>,
    pub reasoning_policy: ReasoningPolicy,
    pub cache_write_policy: CacheWritePolicy,
}

#[derive(Debug, Clone)]
pub struct Rates {
    input: i64,
    cached_input: i64,
    cache_write: i64,
    output: i64,
    reasoning: i64,
    reasoning_policy: ReasoningPolicy,
    cache_write_policy: CacheWritePolicy,
}

fn rate(value: &str, field: &'static str) -> Result<i64, Error> {
    let invalid = || Error::InvalidRate { field };
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    if whole.is_empty()
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || !fraction.bytes().all(|b| b.is_ascii_digit())
        || fraction.len() > 6
        || (value.contains('.') && fraction.is_empty())
    {
        return Err(invalid());
    }
    let mut units = 0i64;
    for digit in whole.bytes().chain(fraction.bytes()) {
        units = units
            .checked_mul(10)
            .and_then(|n| n.checked_add(i64::from(digit - b'0')))
            .ok_or_else(invalid)?;
    }
    units
        .checked_mul(10i64.pow(6 - fraction.len() as u32))
        .ok_or_else(invalid)
}

impl PriceInput {
    pub fn validate(&self) -> Result<Rates, Error> {
        let reasoning = match (self.reasoning_policy, self.reasoning.as_deref()) {
            (ReasoningPolicy::Separate, Some(value)) => rate(value, "reasoning")?,
            (ReasoningPolicy::Separate, None) | (_, Some(_)) => return Err(Error::ReasoningRate),
            (_, None) => 0,
        };
        Ok(Rates {
            input: rate(&self.input, "input")?,
            cached_input: rate(&self.cached_input, "cachedInput")?,
            cache_write: rate(&self.cache_write, "cacheWrite")?,
            output: rate(&self.output, "output")?,
            reasoning,
            reasoning_policy: self.reasoning_policy,
            cache_write_policy: self.cache_write_policy,
        })
    }
}

impl Rates {
    /// Research component prices: reasoning uses output's price unless a separate
    /// reasoning price was configured. This does not change normal valuation policy.
    pub(crate) fn hypothesis_value(&self, components: [i64; 5]) -> Result<i128, Error> {
        let reasoning = if self.reasoning_policy == ReasoningPolicy::Separate {
            self.reasoning
        } else {
            self.output
        };
        components
            .into_iter()
            .zip([
                self.input,
                self.cached_input,
                self.cache_write,
                self.output,
                reasoning,
            ])
            .try_fold(0i128, |sum, (tokens, rate)| {
                i128::from(tokens)
                    .checked_mul(i128::from(rate))
                    .and_then(|amount| sum.checked_add(amount))
                    .ok_or(Error::Overflow)
            })
    }

    pub fn value(&self, tokens: &Tokens) -> Result<i128, Error> {
        let values = tokens.values().ok_or(Error::MissingCategories)?;
        let [input, cached, writes, output, reasoning, _total] = values;
        if values.iter().any(|n| *n < 0) || cached > input || reasoning > output {
            return Err(Error::InvalidCategories);
        }
        let mut ordinary_input = input - cached;
        match self.cache_write_policy {
            CacheWritePolicy::Unknown if writes != 0 => return Err(Error::UnknownCacheWrite),
            CacheWritePolicy::IncludedInputDisjoint => {
                ordinary_input = ordinary_input
                    .checked_sub(writes)
                    .filter(|n| *n >= 0)
                    .ok_or(Error::InvalidCategories)?;
            }
            _ => (),
        }
        let (ordinary_output, separate_reasoning) = match self.reasoning_policy {
            ReasoningPolicy::Unknown if reasoning != 0 => return Err(Error::UnknownReasoning),
            ReasoningPolicy::Separate => (output - reasoning, reasoning),
            _ => (output, 0),
        };
        [
            (ordinary_input, self.input),
            (cached, self.cached_input),
            (writes, self.cache_write),
            (ordinary_output, self.output),
            (separate_reasoning, self.reasoning),
        ]
        .into_iter()
        .try_fold(0i128, |sum, (tokens, rate)| {
            i128::from(tokens)
                .checked_mul(i128::from(rate))
                .and_then(|amount| sum.checked_add(amount))
                .ok_or(Error::Overflow)
        })
    }
}
