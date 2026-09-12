# Selecting time and inspecting model calls

Dashboard has one time interval for quota observations, local tokens, estimated
cost, models, projects, turns, and invocation inspection. Drag left mouse to
select, Ctrl + wheel to zoom around the pointer, and Shift + drag to pan. Back
undoes navigation; Reset zoom restores the period before that navigation began.
Custom dates use the device's local zone. A selected interval stays fixed; a
trailing duration follows the clock. The retained history remains about 45 days.

`usage_dashboard` keeps existing range presets and additionally accepts
`{range:"custom",start,end}` and `{range:"trailing",durationSeconds}`. Times are
`{seconds,nanos}`. Custom bounds must satisfy start < end <= query clock; duration
must be positive. Modes reject unrelated bounds. The response adds
`availableStart`, `availableEnd` (actual retained history, independent of the
selected interval) and `rangeQuota` with the last selected observation, separate
comparable segment estimates (latest 50), segment count, recent estimate and
unmatched edge cost. Historical chart projections keep their original baselines.
An interval estimate uses actual quota endpoints inside the selection; it never
divides all selected usage by a denominator covering only part of the selection.

`usage_calls` accepts `{start,end,model?,thread?,after?,limit?}`. Model and thread
are exact IDs; omitted filters mean all. Limits are 1–50 and default to 50.
Calls are accepted observations ordered by exact timestamp and durable ID; one
turn can contain many calls. The exclusive cursor is bound to the interval and
filters. Every page includes totals over the whole filtered selection, not just
its rows. Refreshes reset paging. Each query uses a consistent read transaction;
different requests are live snapshots and can observe new data.

Time membership is `start < usage timestamp <= end`. Source counters include
overlapping cached input and reasoning. Billed category quantities and prices
use the preserved price version, with ordinary input excluding cached input and,
where that version requires it, cache writes. Output includes reasoning. No
historical valuation is rewritten; unknown prices remain unknown.

`usage_call_activity` accepts `{observationId,start,end,cursor?}`. It resolves the
stored source, validates generation, file identity, and the original usage
record, and reads a captured file size in cooperative batches of at most 4 MiB
and 100 returned events. It returns progress, notices and a continuation even
when the current scan found no visible events. Closing the call or changing
range stops client continuations. Up to eight pending scans and 2048 text
references are cached in memory; expired references can be refreshed by reopening
the call. Log records above 2 MiB and incomplete records produce coverage notices.
Neither message contents nor raw log copies are persisted.

Generated items in the same turn between usage markers are associated by log
order, explicitly labelled as such. Conflicting generation phases remain
ambiguous. Tool results join by call IDs; completion metadata joins by item IDs.
Commands inside exactly one open tool batch can be associated to that batch,
with a separate association label. Incoming inter-agent messages are context,
not generated model output. Compaction can link directly by response ID; its
embedded usage mirror is never another charge. Unknown activity linkage does
not affect the price of the accepted invocation.

Activity has its own timestamp filter: content outside the selected interval
is hidden even when its invocation was selected. Partial calls are labelled.
File changes show recorded paths, status and available diffs. Shell paths are
inferences from command arguments, not proof of filesystem accesses. The viewer
never runs commands or interprets message text as instructions.

`usage_activity_text` accepts `{textRef,offset?}` and reads the selected field in
UTF-8-safe chunks of at most 64 KiB, with `nextOffset` until complete. It checks
both source identity and the activity record's hash before returning content.
Missing/replaced files leave accounting available and explain missing activity.
Only text actually present in the log can be shown; this does not reconstruct a
complete historical API request or unavailable encrypted reasoning.

Checks: `cargo test --manifest-path src-tauri/Cargo.toml --lib`,
`npm run build`, `node tests/dashboard-ui.mjs`, and
`node tests/call-inspection-ui.mjs` (set `PLAYWRIGHT_MODULE` if needed).
