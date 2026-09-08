# Weekly quota hypotheses

The Dashboard preserves local activity, estimated cost, breakdowns and Weekly
quota. Below these, one shared chart and eight compact cards compare a mandatory
Input + Output base with every subset of Cached input, Cache writes and Reasoning.
These are research hypotheses, not OpenAI billing rules or subscription prices.

`usage_dashboard.quotaAnalysis` returns the latest 512 disjoint comparable
intervals within the selected range and the total interval count. Each interval
starts at a trustworthy quota observation and ends at the first observation at
least one percentage point higher. Usage matches `start < timestamp <= end`.
Resets, ambiguous observations and unfinished intervals are excluded. No data
is interpolated across a boundary. Empty local intervals have unknown coverage.

Each interval adds `hypotheses`: 16 results (eight optional masks for each of the
two cache-write interpretations). A result contains `mask` (cached input=1,
cache writes=2, reasoning=4), `writesIncluded`, exact integer `tokens`, exact
integer trillionths of USD `estimatedUsd`, and `tokenReason` / `priceReason`.
Null amounts mean unavailable, never free. The frontend displays eight results
for the chosen interpretation. Aggregate `tokens` remain available in the
contract but are not used to calculate hypothesis amounts.

For each accepted usage observation, let I=input, C=cached input, W=cache writes,
O=output, R=reasoning and bC/bW/bR be the selected optional bits. Let d=1 for
writes included in input, otherwise 0. The non-overlapping selected components are:

```
U = I - C - d*W
V = O - R
T = U + V + bC*C + bW*W + bR*R
A = (U*pInput + V*pOutput + bC*C*pCached + bW*W*pWrite + bR*R*pReasoning) / 1,000,000
```

Prices are model-specific USD per million tokens. Reasoning uses a separately
configured reasoning price, or the output price when no separate price exists.
The research write interpretation is explicit and independent of normal
Estimated token cost semantics. Input and Output badges mean U and V.
Required missing counters, negative counters or impossible subtractions make
the result unavailable. Validation happens per observation, so a bad subset
cannot be hidden by aggregation. A missing write counter is required when writes
are selected or subtracted, but not for an additional-write baseline.

The storage projection joins each observation to its immutable valuation's
price version. For observations without a valuation, it uses the applicable
effective-time version, or the explicitly permitted first-price backfill.
Subsequent prices never replace an existing valuation version. No current-price
or averaged-model shortcut is used. Missing applicable prices invalidate the
entire interval's monetary amount while preserving usable token results.
This follows [the preserved-history ADR](adr/0001-preserve-priced-history.md).

For a hypothesis's usable intervals j with observed percentage points qj:

```
weighted tokens / 1% = sum(Tj) / sum(qj)
API-equivalent USD / 1% = sum(Aj) / sum(qj)
API-equivalent USD / 100% = 100 * sum(Aj) / sum(qj)
```

If any usable interval is monetarily incomplete, the monetary card metrics are
unavailable; the priced subset is not presented as a complete average. Cards
report usable / retained interval counts. Spread is population standard
deviation / mean of token ratios (CV), with at least three usable intervals.
Exact integer arithmetic is retained until display rounding (half up, two
decimals for tokens, six for USD). Chart coordinates and spread are approximate.

USD is the default chart mode; a keyboard-accessible segmented control switches
to tokens. Stable colors link the chart and card controls. Independent dots
never connect across interval boundaries. Hover or focus the chart and use
Left/Right, Home/End to read interval bounds, percentage increase and both metrics.
The collapsed Method and data quality block contains formulas and interpretation.

Verification: Rust domain tests cover all masks, both write interpretations,
component rates, missing/invalid counters and unpriced usage. Storage tests cover
mixed models and versions, explicit initial backfill, preserved history across
restart, resets and ambiguity. `tests/dashboard-ui.mjs` checks eight cards,
series visibility, stable colors, keyboard controls, metric switching and
responsive layout alongside the preserved upper Dashboard behaviors.
