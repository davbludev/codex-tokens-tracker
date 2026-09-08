# Settings and CSV export

Settings uses the same automatic discovery as ingestion: `CODEX_HOME`, then
`.codex` under the user profile/home. A standard installation needs no override.
To switch sources, choose an absolute Codex **home** containing a readable
`sessions` or `archived_sessions` directory. The application validates and prepares
the replacement watch before saving. Imported history and source checkpoints
remain in the same database; returning to a directory or importing copied records
does not add the same accepted usage again. A saved source that temporarily
disappears remains selected and can recover through the existing parent watch.

Model Pricing opens the existing price editor. Startup and tray preferences apply
when saved and are restored when the tracker starts. All three default to off.
See [desktop monitoring](desktop-monitoring.md) for tray actions and startup behavior.

Diagnostics reports the SQLite database path, combined database and WAL size in
bytes (retiring usage older than 45 days frees pages for reuse, so the size stops
growing rather than shrinking), observed session count excluding missing-parent placeholders, normalized
usage record count (including pending/rejected records), the monitored directory,
source availability, and concise metadata-only errors. Last successful ingestion
is the local time when a complete-line batch committed, not the timestamp of an
old usage observation. It survives restart; empty scans and incomplete tails do
not advance it. Pre-upgrade ingestion time is unavailable until another batch
commits. Refresh diagnostics to read the latest counts.

CSV export writes a **new absolute `.csv` path** in an existing writable folder.
Existing files are never replaced. Work runs on a background worker and streams
one row at a time from a stable read-only SQLite snapshot; sorting/grouping can
spill to disk. An export can therefore coexist with new ingestion, but its values
reflect its starting snapshot. Export errors are shown beside the destination.

| Report | Scope |
| --- | --- |
| Sessions | Each observed session's direct usage, project attribution, and estimated token cost. |
| Model usage | Direct usage grouped by attributed model, with an explicit unknown model bucket. |
| Project totals | Direct usage grouped by project, counting each session once. |
| Weekly cycles | Completed and current observed core weekly cycles, with comparable-interval local tokens/cost and account-wide percentages. |

Headers include units and scope. Known token subtotals have accompanying
completeness states; overlapping token categories must not be added together.
Estimated cost is exported as decimal USD, retaining all 12 fixed-point decimal
places. Blank unknown money and explicit unpriced/incomplete/unavailable states
must not be treated as zero. Weekly estimates retain the existing comparable
observation rules and do not attribute account percentages to individual sessions.
CSV quoting preserves commas, quotes, and newlines in allowed metadata. Prompts,
messages, and raw logs are never exported.
