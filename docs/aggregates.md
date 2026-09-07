# Prepared usage queries

`usage_aggregates` accepts a `query` and returns prepared Rust aggregates. The
existing `usage_snapshot` command and `usage-updated` event are unchanged. No
price configurations, call counts, observation lists, or raw source records are
delivered.

Query kinds are `global`, `session` (with `thread`), `sessions`, `projects`,
`models`, `children`, `ancestors`, and `sessionList` (described below). Children
and ancestors also require `thread`. The original list kinds require
`page: { limit: 1..50, after: null | nextCursor }`. Original session lists
and ancestor lists sort by session ID, not traversal depth; group lists sort by
group ID. Children are immediate effective children. A missing session query
returns null. Empty lists have no next cursor.

Each page's `totalItems` and `direct` summary cover the entire selection,
independent of cursor and page size. Project/model pages summarize global direct
usage; their individual groups also contain direct usage only. Global usage
counts each accepted observation once. Inclusive session usage follows unique
reachable sessions, including the session itself, and is never fed into group
or global totals. Unknown model and unattributed project groups use `unknown:`.
Sessions with no observations remain visible as unavailable, including in the
unknown model group. Placeholders appear in session/tree queries, with no own
accepted subtotal or project membership; their descendants can have usage.

Group IDs prefix the exact observed identity with `model:`, `location:`, or
`repository:`. The attribution basis distinguishes an observed model, a
location-derived bucket and a confirmed Git common directory. Confirming or
invalidating identity can legitimately regroup sessions. These IDs are not
repository URLs, titles, or inferred membership. Missing/ambiguous metadata is
unattributed.

All token values are decimal strings. Each category reports `knownTokens` and
`complete`; null means no known subtotal and false means some accepted usage
lacks the category (or no accepted usage exists). Missing and explicit null
categories both remain unavailable. Category completeness concerns accepted
observations only, not history completeness. Coverage separately reports
unavailable/incomplete sessions, unresolved usage, unknown model/project
attribution, and source diagnostics. Cached input and reasoning can overlap
input/output, and cache-write overlap is not established: do not sum categories.

Every summary also contains `estimatedCost: { knownSubtotal, complete }`.
`knownSubtotal` is a canonical integer string in 10^-12 USD, summed exactly from
immutable valuations of accepted observations. It is an estimated token cost,
not an actual charge. Reads never apply current prices to historic usage.
`complete` is true only when the scope has accepted usage and every accepted
observation has a valuation. Empty or entirely unpriced scopes return null/false;
priced zero returns `"0"`. A non-null subtotal with false completeness is an
explicitly incomplete known cost subtotal. This applies equally to direct,
inclusive, global, group and full-selection page summaries. Missing prices or
unsupported pricing semantics retain tokens and do not imply zero cost. Cost
completeness concerns accepted usage, independently of the coverage fields.
Model grouping follows current observation attribution, while the amount retains
its original valuation even if later metadata changes the model attribution.

Each response is one consistent read transaction. `hierarchyPending` and
`hierarchyRevision` accompany the result. During reconciliation, session direct
usage is still returned, while inclusive usage and effective parents are
unavailable. Children/ancestor queries return `hierarchyPending`; global and
group queries continue. Settled session `parentState` preserves ambiguity,
legacy, self/cycle and missing-parent evidence from the resolver.

`observedAt` is the latest parseable source observation time in that summary,
including unresolved observations; it is not an import-completion timestamp.
Subsequent pages may observe new ingestion or regrouping. Callers refresh a list
when its data changes; cursors do not promise a frozen multi-request snapshot.

The runtime resumes durable pricing jobs at startup in bounded batches alongside
ingestion. Every processed batch, including job completion without observations,
marks the existing `usage-updated` publication dirty; aggregate consumers should
refresh on that event. The writer can explicitly request pricing work after a
save; the price-save IPC is a separate delivery area. Drained pricing work adds
no polling or idle wakeups. Pricing storage failures use the existing runtime
failure publication path; absent valuations are normal completeness data.

Queries use a separate read-only SQLite connection on a blocking worker. Only
bounded pages and SQL summaries enter Rust/IPC. The database may scan the full
selected history for totals; no latency SLA or million-observation benchmark is
claimed. Cost sums use a checked i128 SQL aggregate over valuation text. Malformed
valuation amounts, i128 cost overflow, SQLite token integer overflow or storage
failures return `storage` rather than a rounded or fabricated subtotal. Invalid
page sizes return `invalidQuery`.

## Session explorer

`sessionList` takes `query: { limit, offset, sort, search, project, model,
fromSeconds, beforeSeconds }`. Limit is 1–50; offset defaults to zero and sort
defaults to `newest` (alternatives: `usd`, `tokens`). Text filters are optional,
literal substrings with SQLite ASCII case folding, capped at 1,024 UTF-8 bytes.
Search matches session ID; project matches its metadata path; model matches any
accepted observation's model. `Unavailable` matches unknown project/model
attribution, including sessions with no accepted observations. Filters combine
with AND. The current source does not retain titles, so the UI shows
`Untitled: SESSION_ID`; it does not inspect message content for titles.

Date bounds are inclusive `fromSeconds` and exclusive `beforeSeconds` on the
latest accepted observation timestamp. The UI converts inclusive UTC calendar
dates to these bounds. Dates do not truncate a row's lifetime direct totals.
Rows without timestamps remain visible unless a date filter excludes them.

Rows sort by project identity first, then descending requested metric, then
session ID ascending. Newest uses native seconds and nanoseconds. USD uses
only complete costs and compares exact canonical decimal lengths/text; unknown
or incomplete costs sort last, including behind a complete zero. Token totals
are direct accepted usage. `items` is bounded by limit; `totalItems` counts the
filtered selection; `nextOffset` is null at the end. This page intentionally
has no full-selection usage summary and excludes missing-parent placeholders.

Each row includes its project basis, at most eight model names plus a full
distinct count and unknown-model flag, its direct summary, and immediate
observed effective subagent count (null while hierarchy work is pending).
Duration and session weekly percentage impact remain null because the current
sources do not establish them. The detail dialog uses the existing `session`
query for direct/inclusive usage. Full tree exploration is separate work.

Live notifications coalesce into a serialized query and restart at offset zero;
filter changes discard stale responses. Paging is transaction-consistent per
request, not a frozen history snapshot. Only one page is retained in the UI.
The explorer scans history in SQLite for selection/sort metrics and reuses
existing summary and exact-cost aggregation; no persistent chart/list cache is
introduced. Browser verification: `node tests/sessions-ui.mjs`, using
`PLAYWRIGHT_MODULE` to locate an external Playwright installation if necessary.
