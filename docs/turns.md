# Turns by model and reasoning

The dashboard's **Turns by model and reasoning** section answers "how much work
did I actually run, on which model, at which reasoning effort". `usage_dashboard`
carries it in `turnActivity`, over the same range as the local usage charts.

## Where reasoning effort comes from

A `turn_context` record names both the turn's `model` and its `effort` (the
source's own wording — `low`, `medium`, `high`, `xhigh`). Effort is stored beside
the model on `turn_contexts` and denormalized onto each observation at insert,
exactly as the model already was.

Model and reasoning conflict independently: two turn contexts disagreeing about
the model make only the model unavailable, and disagreeing about the effort makes
only the effort unavailable. Each records its own source diagnostic.

Codex writes no `turn_context` for a turn it starts on its own, such as context
compaction. That turn still ran under the thread's prevailing settings, so its
usage inherits the model and effort of the latest earlier attributed observation
of the same thread, whatever order sources were imported in. A recorded context
is always the turn's own evidence and is never overridden — including where a
disagreement already erased one of its attributes, which stays unavailable.
Unattributed usage can never be valued, and one such turn used to leave the
weekly cost-per-percentage-point estimate unavailable for the rest of the cycle.
Schema version 14 applies the same rule once to usage already stored without
attribution, and values it; durable valuations are never rewritten.

Effort was added in schema version 11. Turns imported before that carry no
effort and report reasoning as unavailable; history is never re-imported to
invent one.

## Counting

A turn is one `turn_id` — the marker Codex writes on each exchange — within
one session. Every turn is attributed to the
bin of its **first** accepted observation in the range, and all of that turn's
tokens and estimated cost are counted in that same bin. One binning rule for all
three metrics means the bars add up to the row beside them whichever metric is
selected, and no turn is counted twice for straddling a bin boundary.

An observation whose record carries no turn marker stands as its own turn rather
than merging with every other unmarked observation in its session;
`turnsWithoutIdentity` reports how many turns came from such records.

## Payload

`turnActivity` has `start`, `end`, `binCount`, `totalTurns`, `combinations`
(distinct combinations before folding), `turnsWithoutIdentity`, a `coverageNote`,
and `series`. Each series has `key`, `label`, `model`, `effort`, `kind`, `turns`,
`acceptedObservations`, `observedSessions`, `tokens`, `estimatedCost`, and
`points` (one per non-empty bin, each with `turns`, `tokens` and
`estimatedCost`).

Sessions are counted distinctly over the whole range, never per bin: a
combination whose turns are spread over a week would otherwise report only the
sessions active inside its busiest bin. Two combinations can share a session, so
their counts cannot be added; the folded `other:` series reports no session count
at all, and the table shows an em dash for it. The same rule applies to the
folded row of the cost table.

Series rank by turns, then by tokens, then by key. The seven busiest are kept and
the rest fold into one `other:` series whose counts and amounts are the exact
sums of what it replaces. A combination that attributes neither a model nor an
effort keeps its own `unattributed` series at the end, as in the cost table:
folding it away would hide the gap. Bin resolution is capped at 96 so the bars
stay readable.

## Presentation

A metric toggle switches the stacked bars between turns, tokens and estimated
cost. Unpriced turns contribute nothing to the cost bars and the panel says so;
when nothing at all is priced the chart is replaced by a prompt to configure
prices rather than a wall of zeroes.

Below the chart, one row per combination reports turns, share of turns, tokens,
tokens per turn, estimated cost, cost per turn and sessions. The per-turn columns
are what separate "expensive per turn" from "merely busy". Shares use largest
remainder so they add up to exactly 100%.

Verification: `turn_activity_counts_turns_per_model_and_reasoning_and_bins_them_once`
covers ranking, labels, multi-observation turns landing in one bin, an
observation with no turn identity and per-bin totals adding up;
`turn_activity_keeps_the_model_when_only_the_reasoning_context_conflicts` covers
independent conflicts; `turn_activity_folds_the_remainder_into_one_series` covers
the `other:` fold. `turn_activity_counts_a_combination_s_sessions_across_every_bin_it_appears_in`
and `folded_rows_report_no_session_count_rather_than_a_wrong_one` cover the
session rules. `tests/dashboard-ui.mjs` checks the stacked series order and
data, all three metrics, the table, the folded and unattributed rows and the
keyboard readout.
