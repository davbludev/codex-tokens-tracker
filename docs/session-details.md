# Session detail delivery

Issue [#10](https://github.com/davbludev/codex-tokens-tracker/issues/10), under
[map #1](https://github.com/davbludev/codex-tokens-tracker/issues/1), extends the
existing `usage_aggregates` command. The wire definitions for the interface are
in `src/sessions-types.ts`; Rust contracts and projection are in
`src-tauri/src/aggregates/session_detail.rs`. Persistence reads live in
`src-tauri/src/storage/aggregates/session_detail.rs`.

## Session and hierarchy

`{kind: "session", thread}` replies with `data.kind: "session"` and a nullable
SessionDetail. A missing ID returns null. `title`, `startedAt`, `endedAt`, and
`durationSeconds` are nullable and currently always null: ingestion establishes
neither title nor lifecycle. `firstObservedAt` and `lastObservedAt` are timestamp
strings from the first/last accepted observations having parsed seconds and
nanoseconds. They do not establish start/end/duration. Placeholder sessions have
no invented observation bounds or direct usage. Weekly percentage impact remains
unavailable; no account percentage is allocated to sessions.

`{kind: "children" | "ancestors", thread, page: {after, limit}}` reuses the existing
hierarchy reader and replies with `data.kind: "sessions"`, an AggregateSessionPage.
Limits are 1–50; null cursor starts a page. Expand children at any depth by their
IDs. Parent `direct` excludes descendants; `inclusive` includes each effective
descendant once. The page's `direct` is the whole selected membership's direct
summary, not a sum of inclusive rows. Ancestors sort by ID, **not breadcrumb
order**; use `parentThreadId` to establish relationships. While hierarchy is
pending, session direct data, models and timeline remain available, inclusive is
null, parent state is `pending`, and Children/Ancestors return `hierarchyPending`.

## Models

`{kind: "sessionModels", thread, page: {after: null, limit: 50}}` replies with
`data.kind: "sessionModels"`, a nullable SessionModels. Its `scope` is `direct`.
Rows sort by attribution ID and page with an exclusive ID cursor. `totalItems`
counts all model buckets and `direct` supplies the **whole-session** denominator
on every page. Empty observed sessions retain the existing unavailable model
bucket convention; placeholders without observations have no rows.

Each row contains attribution, a direct Summary with all independent token
categories and exact estimated cost, `costShare`, and
`costShareUnavailableReason`. Current observation attribution owns the bucket,
including `unknown:`. Costs come only from immutable observation valuations;
an attribution conflict can move retained priced usage to the unknown bucket
without revaluing it. Categories overlap and must not be added together.

`costShare` is a decimal percentage string rounded **half-up to two places**
using exact integers. The row and whole-session costs must both be complete,
with a positive whole-session denominator. Null shares carry `unavailable` when
either cost has no known subtotal, otherwise `incomplete` when either cost is
incomplete, otherwise `zeroDenominator` for a zero whole-session cost. A complete
zero-cost row under a positive complete denominator has `"0.00"`. Independently
rounded shares need not sum to exactly 100.00. Never recalculate page-local shares.

## Timeline

`{kind: "sessionTimeline", thread, pointBudget: 512}` replies with
`data.kind: "sessionTimeline"`, a nullable SessionTimeline. Omitted pointBudget
defaults to 512; allowed budgets are 8–4096. The timeline is direct usage across
all accepted observed history, independent of weekly quota. `timeSource` is
`acceptedObservationTimestamp`; bounds are nullable `{seconds, nanos}` objects.

Equal seconds/nanoseconds are grouped before accumulation. Each retained point
contains exact cumulative `cumulativeTotalTokens` (Category) and
`cumulativeEstimatedCost` (EstimatedCost, integer trillionths of USD). Missing
values are null; known subtotals remain explicitly incomplete after any gap.
There is no synthetic zero, reset, or fabricated lifecycle endpoint.

Draw each series only across its own `tokensConnectFromPrevious` or
`costConnectFromPrevious` connection. Missing-token and unpriced groups break the
respective series on entry and resumption. Subsequent known increments can
connect while their cumulative subtotal remains labelled incomplete. A point at
a missing group can still carry an earlier known subtotal; its disconnected
value must not be presented as complete usage for that timestamp.

The backend streams grouped rows through at most `floor(pointBudget / 2)` bins,
retaining first and last actual points in each occupied bin. Cumulative
nonnegative extrema are those endpoints. First/last observed points survive;
returned points never exceed the budget. Boundary summaries retain bin index,
first/last boundary times, a timestamp-group count, distinct kinds, and
`overloaded`. Multiple boundary groups overload a bin; connections within and
out of that bin conservatively break even if both retained endpoints look known.
Boundary kinds are observationStart, missingTokens, tokensResumed, unpricedUsage,
and pricedUsageResumed. No unbounded boundary list crosses IPC.

`sourceObservationCount` counts timed accepted rows, `sourcePointCount` counts
distinct timestamp groups before sampling, and `returnedPointCount` counts
retained points. `untimedObservationCount` counts accepted rows lacking either
parsed time component. Untimed usage contributes to `direct` totals but cannot
be placed on the timeline. Explain this difference next to the graph; coverage
also comes from `direct.coverage` and `coverageNote`.

## Freshness, errors and validation

Each aggregate request uses one consistent read transaction. Separate requests
and subsequent pages are live snapshots, not an atomic multi-request view.
Consumers should reset paging on live changes, guard against stale replies after
session navigation, and show loading, unavailable, and error states explicitly.
Errors remain `invalidQuery`, `hierarchyPending`, or `storage`. Existing read-only
delivery and metadata-only persistence remain unchanged; there are no migrations
or dependencies added for details.

Focused coverage lives in `src-tauri/src/tests/aggregates/session_detail.rs`.
Run `cargo test --manifest-path src-tauri/Cargo.toml --lib tests::aggregates::` for
the detail checks and existing session-list, direct/inclusive, pricing,
readiness and paging regressions. UI accessibility, full-suite validation and
native delivery verification belong to the integrating area.
