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

**Estimated token cost**:
The valuation of token usage at user-configured prices, rather than an actual charge.
_Avoid_: Actual cost, amount charged, bill

**Unpriced usage**:
Usage whose estimated token cost is unknown because no applicable configured price covers it.
_Avoid_: Free usage, zero-cost usage

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

**Comparable observation interval**:
An interval bounded by trustworthy rate-limit observations and the estimated token cost for the same period, excluding newer cost that has no matching limit observation.
