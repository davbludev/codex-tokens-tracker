# Value each observation once with a bounded reach-back, and retire old usage

Supersedes [0001](0001-preserve-priced-history.md).

Estimated token cost values local usage at user-configured prices. Under the previous decision, usage recorded before a model's first price stayed unpriced unless the user explicitly backfilled it, so dashboards ignored most of a day's activity whenever prices were configured late. Priced history was also kept forever, so the local database only grew.

Every accepted observation is now valued exactly once, with the price version nearest to it: the latest version effective at or before the observation, else an explicit or first-price backfill, else the model's earliest later version when that version becomes effective within seven days of the observation (the valuation reach-back). A saved valuation never changes afterwards; later price edits create versions for later usage. The upgrade to schema 10 queues one valuation pass per priced model so existing unvalued history within the reach-back is valued once.

Usage older than 45 days before the newest stored time is retired: observations, their valuations and weekly limit samples are deleted in bounded batches every six hours. A valuation can be deleted only together with its observation. A stored retention floor stops replayed source files from re-importing retired history.

## Consequences

- Totals, breakdowns and cost per weekly percentage point cover only retained usage; the oldest retained weekly cycle can be truncated at its start.
- Unpriced usage remains unknown rather than zero. Usage more than seven days before a model's first price is still unpriced until the user backfills it explicitly.
- Sessions whose usage was fully retired remain as metadata-only sessions; freed SQLite pages are reused rather than returned, so the file stops growing rather than shrinking.
- The retention cutoff is anchored to the newest stored time and never to a clock ahead of it, so a wrong system clock cannot empty history.
