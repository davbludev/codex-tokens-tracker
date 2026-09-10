# Codex Usage Tracking

This context describes locally observed Codex session usage and its valuation using user-configured token prices, alongside account-wide weekly usage percentages.

## Language

**Direct session usage**:
The tokens attributable to a session itself, excluding its descendants.
_Avoid_: Total session usage

**Inclusive session usage**:
The tokens attributable to a session and all of its descendant sessions.

**Global usage**:
The aggregate of locally observed session usage across projects, counting each session once.

**Project usage**:
The aggregate of locally observed session usage attributed to a project, counting each session once.

**Location-derived bucket**:
A grouping supported by one unambiguous observed workspace root or working directory; it does not by itself confirm repository identity.

**Confirmed repository identity**:
A shared canonical Git common directory established by local administrative path metadata, including across worktrees.

**Effective parent**:
A parent supported by agreeing evidence whose relationship is neither self-referential nor part of a cycle. An unresolved parent's own ancestry does not invalidate its descendants.

**Unreadable record**:
A source record the bounded reader could not buffer, so its type was never
interpreted. It is skipped rather than stopping the source: usage it may have
carried stays unavailable because the records after it cannot bridge the
thread's counters, and is never derived from their difference.
_Avoid_: Corrupt record, lost usage

**Spawn context**:
The metadata of the session that spawned a subagent, replayed into the
subagent's own source. It establishes the parent it already named, never a
second identity for that source and never an observation of the parent's own
session.
_Avoid_: Conflicting identity, second session

**Missing-parent placeholder**:
A session referenced by parent evidence that has not itself been observed; it carries no invented usage or project membership.

**Turn**:
One turn identity within one session. It is counted once, in the bin of its first accepted observation in a range, where its tokens and estimated token cost are counted too.
_Avoid_: Request, API call, message

**Reasoning effort**:
The effort a turn's own context recorded for it, in the source's own wording. It is unavailable for turns observed before this application began storing it, and for a turn whose contexts disagree.
_Avoid_: Reasoning level, thinking budget

**Model and reasoning combination**:
One observed model paired with one observed reasoning effort. An unavailable model and an unavailable effort never merge into a named combination.

**Unattributed combination**:
The combination of turns that attribute neither a model nor a reasoning effort. It keeps its own row rather than folding into the remainder.

**Estimated token cost**:
The valuation of token usage at user-configured prices, rather than an actual charge.
_Avoid_: Actual cost, amount charged, bill

**Unpriced usage**:
Usage whose estimated token cost is unknown because no applicable configured price covers it, including usage more than the valuation reach-back before a model's first price.
_Avoid_: Free usage, zero-cost usage

**Valuation reach-back**:
The seven days before a price version's effective time during which earlier usage of that model, if no other price applies, is valued once with that version.

**Retired usage**:
Observations, their valuations and weekly limit samples older than 45 days before the newest stored time, removed from local storage and never re-imported.
_Avoid_: Deleted history, purged data

**Estimated cost by token category**:
The split of a scope's estimated token cost into uncached input, cached input, cache-write and output amounts, each observation at its own model's price version; the four amounts add up to that scope's estimated token cost.

**Uncached input cost**:
The estimated token cost of input tokens that were not served from cache, and that a price version does not also count as a cache write.
_Avoid_: Input cost

**Per-model estimated cost**:
The estimated token cost by token category of one model's direct usage within a range, computed only from observations that already carry a durable valuation, so it reconstructs that model's known cost subtotal exactly.

**Folded model remainder**:
One row standing for the models outside the ranked rows, whose tokens and amounts are the exact sums of the models it replaces; it is not a model.

**Blended cost per million tokens**:
One model's known cost subtotal divided by its total tokens; it mixes the four category prices and the observed cache-hit ratio, and is not a configured price.
_Avoid_: Model price, rate

**Known cost subtotal**:
The sum of estimated token costs for priced usage within a scope; it is incomplete when that scope also contains unpriced usage.

**Price version**:
A user-configured token price with an effective time that distinguishes it from earlier and later prices.

**Observed cost per weekly percentage point**:
The ratio of observed local estimated token cost to account-wide weekly usage percentage points; it does not establish complete account usage.
_Avoid_: Actual cost per percent, complete account cost

**Observation interval**:
The interval beginning with the first trustworthy comparable observation of both estimated token cost and account-wide weekly usage percentage, which may cover only part of a weekly cycle.

**Recent cost per weekly percentage point**:
The observed cost per weekly percentage point over the last 15 minutes, when comparable observations are sufficient.

**Session weekly percentage impact**:
The weekly percentage consumption independently attributable to a session; it is unavailable when local observations cannot establish that attribution.

**Weekly window**:
The account-wide weekly limit period a sample reports, identified by its reset
time rather than by its percentage. Reset times reported for one window jitter by
seconds; distinct windows are hours apart.

**Stale observation**:
A trustworthy sample that re-reports a snapshot of its own weekly window taken
earlier than one already observed, recognized by a lower percentage within that
window or by a superseded reset time. It is ignored rather than counted.
_Avoid_: Decrease, reset, rollback

**Detected reset**:
The start of a cycle established by a reported weekly window that supersedes the
current one, or, where no window was ever reported, by a strict decrease.
_Avoid_: Cycle boundary, actual reset

**Comparable observation interval**:
An interval bounded by trustworthy rate-limit observations and the estimated token cost for the same period, excluding newer cost that has no matching limit observation.
