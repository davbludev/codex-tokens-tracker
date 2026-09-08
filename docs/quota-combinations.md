# Subscription token combinations

The dashboard includes a price-independent comparison of tokens per observed
weekly percentage point. `usage_dashboard.quotaAnalysis` contains the latest
512 disjoint intervals inside the selected range and the total interval count.
Each interval starts at a trustworthy quota observation and ends at the first
subsequent observation at least one percentage point higher. Accepted direct
usage is counted once with `start < timestamp <= end`. The final unfinished
interval is excluded. Resets and ambiguous observations discard unfinished
intervals; nothing is interpolated across a boundary. Missing categories are
unknown, not zero. Empty local intervals have unknown token coverage.

One grouped token stream is merged with the existing canonical quota reducer.
Memory and response size are bounded. No model prices or valuation jobs are
needed; unknown models and codex-auto-review can participate through their
accepted token counters. This does not establish complete account coverage.

Five components produce 31 nonempty combinations: uncached input, cached input,
cache write, visible output, reasoning. The user can compare both hypotheses
about cache write: additional to input, or a disjoint part of uncached input.
In the latter case uncached input subtracts both cached input and writes.
Visible output subtracts reasoning. Algebraic cancellation avoids requiring an
unavailable subset when its containing counter alone is sufficient. Negative
results and contradicted included-write hypotheses are unavailable.

The chart shows up to six combinations as independent dots at interval end
times. The keyboard inspector shows exact matched bounds, percentage points,
token counts and ratios. Selection and write interpretation persist locally.
The statistics table contains all combinations, with weighted tokens per point,
minimum/maximum ratios, usable interval count and coefficient of variation
(population standard deviation / mean, requiring three intervals). Statistics
cover only usable displayed intervals per combination. Ratios use exact integer
arithmetic before rounding to two decimals; numeric coordinates and dispersion
are approximate. The recent filter retains only intervals wholly within the
last 15 minutes and selected dashboard range.

Low variation is not evidence of a provider billing formula. Model mix, quota
rounding, delayed observations, unobserved usage and missing counters affect the
comparison. The monetary valuation remains a separate configured-price estimate.

For this change, verification is a production build and interactive inspection;
automated tests were explicitly excluded by the user.
