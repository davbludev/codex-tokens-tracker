# Weekly observation queries

`usage_weekly` accepts `query: { before: null | nextCursor, limit: 1..50 }`.
It evaluates a consistent read-only SQLite transaction on a blocking worker.
`evaluatedAt` is the query clock, not ingestion time. Refresh on `usage-updated`
and as time advances when displaying the recent estimate. The existing cached
snapshot and event payload are unchanged.

Only the exact `codex` bucket with duration `10080` minutes participates.
Other buckets and durations remain distinct stored metadata. Position and reset
time are metadata, never limit identity or evidence that a reset occurred.
No app API data is read. Session weekly percentage impact is explicitly null;
account-wide percentage is never distributed across sessions by tokens or cost.

Samples sort by parsed source time, independent of ingestion order. Canonically
equal timestamps and numerically equal percentage spellings count once. A strict
decrease starts a cycle even before the expected reset. Missing, invalid, future
timestamps or invalid percentages are retained but excluded, counted by
`excludedSamples`. Decimal input is bounded to 256 bytes and exponent magnitude
1024 before parsing; out-of-range values outside 0..100 are excluded.

Conflicting percentages at one canonical timestamp form an ambiguous barrier.
They neither assert a reset nor permit a comparable interval across the barrier.
The last trustworthy percentage stays available; the next trustworthy sample
starts a new comparable segment within the existing cycle. Conflicting reset
metadata makes reset time unavailable without invalidating an agreed percentage.

`currentCycle` contains the first and latest trustworthy observations, known
reset metadata and ambiguity coverage. Times use exact Unix `seconds` and
`nanos`. `observationAgeSeconds` is the age of the last trustworthy observation.
Cycle keys are canonical first-observation timestamps, deterministic from the
currently retained evidence. Earlier imports can legitimately revise derived
cycles. No cycle table is rewritten or discarded. `history` pages completed
cycles newest first, separately from the current cycle. Pages are bounded, and
their cursors are exclusive; subsequent requests can see new source evidence.

Each history item retains the existing cycle fields at the top level and adds
`estimate` and `tokens`. The estimate is exactly the overall estimate from the
final comparable segment immediately before the next detected reset. An earlier
ambiguity can recover, so `hasAmbiguousObservations` can be true while a narrower
estimate is available. First/last observed cycle bounds are distinct from the
estimate's compared bounds. A one-sample segment has unavailable estimates and
null tokens. Available tokens cover the same start-exclusive/end-inclusive
interval as cost, with the shared category availability semantics (an empty
interval has unavailable token categories but complete zero cost).

`usage_weekly_models` accepts
`query: { cycleKey: string, page: { after: null | nextCursor, limit: 1..50 } }`.
The canonical cycle key is resolved against completed cycles in the same read
transaction as the breakdown; clients cannot supply interval boundaries. It
returns null when the key is no longer a completed cycle, otherwise
`{ cycleKey, estimate, items, nextCursor }`. An existing cycle with no comparable
interval returns its unavailable estimate and an empty model page. Each item
contains `{ id, model, tokens, estimatedCost }`, where `model` can be null.
Groups use accepted observation model attribution and stored valuations for the
matched interval, never historical valuation model names or apportioned quota.
IDs are `model:<name>` and `unknown:`. Binary lexical ordering and an exclusive
cursor yield at most 50 items; SQL groups the selected interval and reads one
extra group to detect a next page. No all-model list is collected in Rust.

Overall estimates use the first and last trustworthy observations in the current
comparable segment. Recent cost per weekly percentage point uses the earliest
and latest trustworthy endpoints within `[evaluatedAt - 15 minutes, evaluatedAt]`
in that same segment and cycle. Both require two distinct endpoint timestamps;
there is no interpolation. A stale last sample remains visible. Its ratio stops
at the matching endpoint, while `unmatchedCost` covers strictly newer accepted
usage through `evaluatedAt`. No sample means no unmatched interval is asserted.

Cost includes each accepted observation exactly once with
`start < usage timestamp <= end`, using stored immutable valuations. It never
reprices historical usage. Observations, valuations and limit samples older
than 45 days before the newest stored time are retired in bounded batches, so
the oldest retained cycle can start at its first retained sample rather than
its true reset; the current cycle is never affected. Subtotals are integer strings in trillionths of USD.
Entirely unpriced usage has null subtotal and false completeness; partially
priced usage has an explicitly incomplete known subtotal. An observed interval
with no local usage has known zero cost and true completeness. Relevant unpriced
usage suppresses ratios, including while durable pricing work remains pending.

Consumed percentage points, remaining percentage and the denominator threshold
are exact decimal calculations. At least exactly one point is required. Prepared
USD ratios are strings rounded directly from the exact rational to 12 fractional
digits using half-even; the full-week estimate is independently calculated as
the unrounded ratio times 100. Rounded display values never drive eligibility.
Unavailable reasons distinguish insufficient observations, ambiguity, a smaller
denominator and unpriced usage. Storage corruption/overflow yields `storage`;
invalid page requests yield `invalidQuery`.

All estimates are labelled “since observation began” and `fullCycleCostKnown`
is false: a detected decrease does not reveal cost between the true reset and
the first sample. These are observed local estimates for the current model mix,
not OpenAI charges or proof of complete account usage. The database can scan and
sort retained core samples; Rust retains only one tie group, current segment
endpoints and a bounded history page. No latency benchmark is claimed.

Focused checks: `cargo test --manifest-path src-tauri/Cargo.toml weekly`.
