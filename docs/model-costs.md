# Cost by model

The dashboard's **Cost by model** section answers "what did each model cost me,
and what did the money go to". `usage_dashboard` carries it in
`breakdowns.modelCosts` together with `breakdowns.categoryTotals`, over the same
selected range as the local usage charts.

## Payload

Each row has `key` (`model:<id>`, `unknown:` or `other:`), `label`, `kind`,
the full `tokens` category set, `estimatedCost`, `acceptedObservations`,
`observedSessions`, and `categories`: exact integer trillionths of USD for
`input`, `cachedInput`, `cacheWrites` and `output`, plus a `reason` that is null
only when all four are available. `categoryTotals` is the same split across
every row, including the folded remainder.

Rows are ranked by estimated cost, then by total tokens, then by key. The eight
most expensive named models are kept; the rest fold into one `other:` row whose
tokens and amounts are the exact sums of the models it replaces, labelled with
how many it covers. Unattributed usage stays in its own `unknown:` row and never
merges with a named model.

## Why the four amounts always add up

Only observations that already carry a durable valuation contribute to the
split, and each is recomputed from the very price version that valuation used,
under that version's policies (reasoning inside output unless separately priced,
cache writes additional to or disjoint from input as configured). The stored
valuation is the checked sum of exactly those four amounts, so the split
reconstructs `estimatedCost.knownSubtotal` exactly. The reader compares the two
before publishing and replaces the split with a reason if they ever disagree.

An observation still waiting for its valuation, or one whose token semantics its
price version cannot interpret, is counted in `tokens` and
`acceptedObservations` but not in the split: `estimatedCost.complete` turns
false and the amounts continue to describe only the known subtotal. A row with
no valued observation at all reports `Unpriced usage: no applicable model price`
and no amounts. Reads never apply current prices to historic usage.

`input` is *uncached* input: it excludes cached input, and also excludes cache
writes when the version treats them as part of input. The token counters keep
their own meaning, where cached input is part of input and reasoning is part of
output; the UI labels them separately and never adds them together.

## Presentation

The section leads with an "All models" strip: the range total, a stacked bar and
one tile per category with its amount and share. Every per-model bar uses that
same scale, so a row's length is its share of total spend and the rows add up to
the strip. Rows also show total tokens, a blended USD per million tokens, and
the sessions the model was used in. Unpriced rows draw no cost segments.

A collapsed disclosure holds the exact table, switchable between estimated cost
per category (with an "All models" footer row) and exact token counts per
category (no footer, because those categories overlap).

Verification: `model_costs_split_each_models_subtotal_by_category_under_its_own_policies`
covers two models with different cache-write and reasoning policies, ranking,
the unknown row and the totals; `model_costs_fold_the_remainder_and_keep_partly_priced_models_honest`
covers the `other:` fold and an accepted observation with no valuation.
`tests/dashboard-ui.mjs` checks the strip, the per-model rows, both table modes
and the unpriced states. `tests/dashboard-preview.mjs` renders the whole
dashboard against a realistic fixture into `docs/preview/` for design review.
