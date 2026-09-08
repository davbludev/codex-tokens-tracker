# Model Pricing

The desktop button opens a native modal HTML dialog. Detected model names load
in bounded pages; a failed page leaves the loaded models available and offers
retry. Reopening or reloading refreshes the catalog. Draft strings survive model
switches, reloads, errors, and closing the dialog for the lifetime of the app.

Prices are USD per million tokens, validated as nonnegative decimal strings
with at most six fractional digits. The server owns validation and effective
time. The form uses one explicitly described monetary convention: reasoning is
included in output, and cache writes are additional to input. These are valuation
assumptions, not subscription quota rules. Historical price versions retain their
original policies; saving uses the displayed convention for the new version.
The dashboard's token-combination comparison requires no prices. Initial backfill is
unchecked by default and offered only before a model has a configured price.
Later saves create future-effective versions. Existing valuations stay immutable.
Each observation is valued once with the nearest price: the latest version
effective at or before it, else a backfill, else the model's earliest later
version when that version becomes effective within seven days (the valuation
reach-back). Usage further back than seven days stays unpriced unless backfilled.
If the initial backfill was skipped, a configured model with older unpriced usage
offers one explicit "Backfill older unpriced usage" action behind a confirmation.
It associates the model's immutable first price with that older usage; it never
creates a version or changes an existing valuation, and it disappears once used.
Usage older than 45 days is retired together with its valuations; see
[the value-once ADR](adr/0002-value-once-and-retire-usage.md).

## Delivery boundary

`pricing_models({ after: string | null })` returns `{ models, nextCursor }`.
The cursor is the exact final model name for a full 64-item page, otherwise null.
Each model contains its latest price version, if any, and `backfillAvailable`,
true only when a configured model still has older unpriced usage that its first
price could cover. Pages do not represent a frozen snapshot; callers deduplicate
model names and restart when refreshing.

`save_model_price({ model, configuration, backfillBefore })` returns the committed
price version. Configuration keys are camelCase, while policy values use
snake_case (`included_input_disjoint`, for example). Rates cross IPC as strings.

`backfill_model_price({ model })` returns the first price version now covering the
model's older unpriced usage and schedules its valuation. It fails with
`backfill_unavailable` when nothing older is unpriced or a backfill already exists.

Both operations use the same bounded inbox as native source events. The sole
writer processes a bounded inbox batch before advancing ingestion/pricing work.
A save commits the version and durable job, requests the pricing lane, then
replies. The normal `usage-updated` publication follows pricing progress. There
is no idle polling or second writable connection.

Errors expose a safe `code`, optional form `field`, and actionable `message`.
Full/disconnected inboxes return `busy`/`unavailable`. Invalid requests fail only
that request; storage internals are not sent to the frontend. The UI never
automatically retries a save. After a connection/storage failure, reload models
before retrying to check whether a version was committed.

## Focused browser verification

`node tests/pricing-ui.mjs` requires an available Playwright package and installed
Microsoft Edge. If Playwright is supplied outside the project, set
`PLAYWRIGHT_MODULE` to its absolute `index.mjs` path. No package is added to the
application. The test starts its own Vite server on port 1421, uses a controlled
Tauri bridge, and closes the server/browser afterward. Set `PRICING_SCREENSHOT`
to an output PNG path to capture the narrow dialog for inspection.

This verifies the browser form/transport contract, not a live native Tauri
session. Backend pricing IPC tests exercise the actual writer request handler,
commit ordering, idle wake, safe errors, and catalog pagination.
